from __future__ import annotations

import os
import re
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from .errors import DirectorError, io_error

# Interpreter and dependency trees that may sit inside a project but never hold user code.
_FOREIGN_DIRS = frozenset({"site-packages", "dist-packages", ".venv", "venv", "node_modules"})


def is_within(path: Path, root: Path) -> bool:
    return path == root or root in path.parents


def public_path(path: Path | str, root: Path) -> str:
    """Project-relative POSIX path when inside `root`, else the absolute path."""

    resolved = Path(os.path.abspath(path))
    if is_within(resolved, root):
        return resolved.relative_to(root).as_posix()
    return str(resolved)


def is_user_source(path: Path, root: Path) -> bool:
    """True for files under `root` that are not part of an installed environment."""

    resolved = Path(os.path.abspath(path))
    if not is_within(resolved, root):
        return False
    return not _FOREIGN_DIRS.intersection(resolved.relative_to(root).parts)


def slug(value: str, fallback: str = "file") -> str:
    return re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-") or fallback


def ensure_dir(path: Path) -> Path:
    try:
        path.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        raise io_error(path, exc) from exc
    return path


@contextmanager
def atomic_target(path: Path) -> Iterator[Path]:
    """Yield a sibling temp path that replaces `path` only if the block succeeds."""

    ensure_dir(path.parent)
    # Keep the suffix last so tools that pick a format by extension (ffmpeg) still can.
    temp = path.with_name(f".{path.stem}.{os.getpid()}.tmp{path.suffix}")
    try:
        yield temp
        os.replace(temp, path)
    except OSError as exc:
        raise io_error(path, exc) from exc
    finally:
        temp.unlink(missing_ok=True)


def create_exclusive(directory: Path, stem: str, suffix: str) -> Path:
    """Create `<stem><suffix>`, or the first free `<stem>-N<suffix>`, without racing."""

    ensure_dir(directory)
    for index in range(1, 10_000):
        name = f"{stem}{suffix}" if index == 1 else f"{stem}-{index}{suffix}"
        candidate = directory / name
        try:
            with candidate.open("xb"):
                return candidate
        except FileExistsError:
            continue
        except OSError as exc:
            raise io_error(candidate, exc) from exc
    raise DirectorError(
        "io_error",
        f"No free file name for {stem}{suffix} in {directory}.",
        {"path": str(directory), "error": "name space exhausted"},
    )
