from __future__ import annotations

import fnmatch
import json
import re
import shutil
import zipfile
from pathlib import Path
from typing import Any, Mapping, Sequence

from .errors import DirectorError
from .util import Emit, confined_path, noop_emit, project_root, run_command


STANDARD_INCLUDES = [
    "director.yaml", "director.yml", "manim.cfg", "README*", "LICENSE*",
    "requirements*.txt", "pyproject.toml", "setup.py", "setup.cfg", "Pipfile", "Pipfile.lock",
    "poetry.lock", "uv.lock", "environment.yml", "environment.yaml", "conda-lock.yml",
    "Dockerfile", "compose.yml", "compose.yaml", "*.py",
    "scenes/**", "src/**", "mobjects/**", "plugins/**", "assets/**", "sources/**",
    "data/**", "audio/**", "captions/**", "narration/**", "themes/**", "fonts/**",
    "expected/**", "output/**", "dist/**", "media/**",
]
DEFAULT_INCLUDES = STANDARD_INCLUDES
DEFAULT_EXCLUDES = [
    "**/__pycache__/**", "**/*.pyc", "**/*.pyo", ".manim-director/**", ".git/**",
    "**/.pytest_cache/**", "**/.mypy_cache/**", "**/.ruff_cache/**", "**/.venv/**",
    "**/node_modules/**", "**/partial_movie_files/**", "**/Tex/**", "**/texts/**",
    "**/.DS_Store", "**/Thumbs.db",
]
PATH_TOKEN = re.compile(r"(?<![A-Za-z0-9_.@+-])(?:[A-Za-z0-9_.@+-]+/)*[A-Za-z0-9_.@+-]+\.[A-Za-z][A-Za-z0-9]{0,15}(?![A-Za-z0-9_.@+-])")
DIRECTORY_KEYS = {"source_dir", "asset_dir", "output_dir"}
DELIVERABLE_EXTENSIONS = {
    ".mp4", ".mov", ".webm", ".gif", ".png", ".jpg", ".jpeg", ".webp",
    ".wav", ".mp3", ".ogg", ".vtt", ".srt", ".zip", ".pdf",
}


def _matches(path: str, patterns: list[str]) -> bool:
    # pathlib-style directory globs often use /** to mean the directory and all descendants.
    return any(fnmatch.fnmatch(path, pattern) or (pattern.endswith("/**") and path.startswith(pattern[:-3].rstrip("/") + "/")) for pattern in patterns)


def _protected_export_path(relative: str) -> bool:
    parts = [part.lower() for part in Path(relative).parts]
    if any(part in {".git", ".manim-director", "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".venv", "node_modules", "partial_movie_files"} for part in parts[:-1]):
        return True
    name = parts[-1] if parts else ""
    if name == ".env" or (name.startswith(".env.") and name not in {".env.example", ".env.sample", ".env.template"}):
        return True
    if name in {".npmrc", ".pypirc", ".netrc", "id_rsa", "id_ed25519", "credentials", "secrets"}:
        return True
    if name.startswith(("id_rsa.", "id_ed25519.", "credentials.", "secrets.", "secret.", "service-account.")):
        return True
    return Path(name).suffix.lower() in {".pem", ".key", ".p12", ".pfx", ".kdbx"}


def _safe_existing_reference(root: Path, raw: str) -> Path | None:
    value = raw.strip().strip("'\"").replace("\\", "/")
    if not value or value in {".", ".."} or "\x00" in value:
        return None
    candidate = Path(value).expanduser()
    if not candidate.is_absolute():
        candidate = root / candidate
    try:
        resolved = candidate.resolve()
        relative = resolved.relative_to(root).as_posix()
    except (OSError, ValueError):
        return None
    if resolved == root or _protected_export_path(relative):
        return None
    return resolved if resolved.exists() and not resolved.is_symlink() else None


def _walk_strings(value: Any):
    if isinstance(value, str):
        yield value
    elif isinstance(value, Mapping):
        for nested in value.values():
            yield from _walk_strings(nested)
    elif isinstance(value, Sequence) and not isinstance(value, (str, bytes, bytearray)):
        for nested in value:
            yield from _walk_strings(nested)


def _manifest_strings(path: Path) -> list[str]:
    if path.stat().st_size > 8 * 1024 * 1024:
        return []
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        return []
    return list(_walk_strings(value))[:20_000]


def _director_strings(path: Path) -> tuple[list[str], dict[str, str]]:
    try:
        text = path.read_text(encoding="utf-8")[:2_000_000]
    except (OSError, UnicodeDecodeError):
        return [], {}
    values: list[str] = []
    directories: dict[str, str] = {}
    try:
        import yaml  # Optional; a lexical fallback keeps the runtime dependency-free.

        parsed = yaml.safe_load(text)
        values.extend(_walk_strings(parsed))
        project = parsed.get("project", {}) if isinstance(parsed, Mapping) else {}
        if isinstance(project, Mapping):
            directories.update({key: str(project[key]) for key in DIRECTORY_KEYS if project.get(key) is not None})
    except Exception:
        pass
    # This also finds paths embedded in fallback commands and works without PyYAML.
    values.extend(match.group(0) for match in PATH_TOKEN.finditer(text))
    for match in re.finditer(r"(?m)^\s*(source_dir|asset_dir|output_dir)\s*:\s*([^#\n]+)", text):
        directories.setdefault(match.group(1), match.group(2).strip().strip("'\""))
    return values, directories


def _default_bundle_includes(root: Path) -> list[str]:
    includes = set(STANDARD_INCLUDES)
    director = root / "director.yaml"
    if not director.exists():
        director = root / "director.yml"
    values: list[str] = []
    directories: dict[str, str] = {}
    if director.exists():
        values, directories = _director_strings(director)
    manifests: list[Path] = []
    seen_files: set[Path] = set()

    def add_reference(raw: str) -> None:
        direct = _safe_existing_reference(root, raw)
        references = [direct] if direct else [_safe_existing_reference(root, match.group(0)) for match in PATH_TOKEN.finditer(raw)]
        for reference in references:
            if reference is None:
                continue
            relative = reference.relative_to(root).as_posix()
            if reference.is_dir():
                includes.add(f"{relative}/**")
            elif reference.is_file():
                includes.add(relative)
                if reference.suffix.lower() == ".json" and reference not in seen_files:
                    manifests.append(reference)
                    seen_files.add(reference)

    for value in values:
        add_reference(value)
    for key, raw_directory in directories.items():
        if key in DIRECTORY_KEYS:
            add_reference(raw_directory)
    # Follow bounded JSON manifests because theme/asset/output manifests often point
    # at the actual fonts, media, sidecars, and other deliverables.
    parsed_manifests = 0
    while manifests and parsed_manifests < 64:
        manifest = manifests.pop(0)
        parsed_manifests += 1
        for value in _manifest_strings(manifest):
            add_reference(value)
    return sorted(includes)


def _explicit_bundle_artifacts(
    root: Path, params: Mapping[str, Any], *, caption_only: bool
) -> list[tuple[Path, str]]:
    """Resolve engine-selected artifacts, including final media in private state.

    The scheduler supplies this field from a successful job result. Render media
    lives below .manim-director/media, which is otherwise excluded wholesale.
    Only regular final deliverables may cross that boundary, and they are stored
    below a neutral archive prefix so no private state layout is exported.
    """

    raw = params.get("artifacts", [])
    values = [raw] if isinstance(raw, (str, Path)) else list(raw) if isinstance(raw, Sequence) else []
    selected: list[tuple[Path, str]] = []
    seen: set[Path] = set()
    for value in values[:200]:
        if not isinstance(value, (str, Path)) or any(token in str(value) for token in ("*", "?", "[")):
            continue
        try:
            path = confined_path(root, str(value), must_exist=True)
        except DirectorError:
            continue
        if not path.is_file() or path in seen:
            continue
        relative = path.relative_to(root).as_posix()
        suffix = path.suffix.lower()
        if caption_only and suffix not in {".vtt", ".srt"}:
            continue
        if _protected_export_path(relative):
            parts = Path(relative).parts
            if (
                len(parts) < 3
                or parts[:2] != (".manim-director", "media")
                or "partial_movie_files" in parts
                or suffix not in DELIVERABLE_EXTENSIONS
            ):
                continue
            archive_path = Path("deliverables", *parts[2:]).as_posix()
        else:
            archive_path = relative
        selected.append((path, archive_path))
        seen.add(path)
    return selected


def _export_zip(params: Mapping[str, Any], emit: Emit, *, caption_only: bool = False) -> dict[str, Any]:
    root = project_root(params)
    default_output = "output/captions.zip" if caption_only else f"output/{root.name}-source.zip"
    destination = confined_path(root, str(params.get("output", default_output)))
    caption_defaults = ["*.vtt", "*.srt", "**/*.vtt", "**/*.srt", "*transcript*.txt", "*narration*.json", "**/*transcript*.txt", "**/*narration*.json"]
    if params.get("include") is not None:
        raw_includes = params["include"]
        includes = [str(raw_includes)] if isinstance(raw_includes, (str, Path)) else [str(value) for value in raw_includes]
    else:
        includes = caption_defaults if caption_only else _default_bundle_includes(root)
    raw_excludes = params.get("exclude", [])
    custom_excludes = [str(raw_excludes)] if isinstance(raw_excludes, (str, Path)) else [str(value) for value in raw_excludes]
    excludes = [*DEFAULT_EXCLUDES, *custom_excludes]
    max_bytes = int(params.get("max_bytes", 2 * 1024 * 1024 * 1024))
    if max_bytes <= 0:
        raise DirectorError("invalid_export_budget", "max_bytes must be positive")
    candidates = []
    selected_paths: set[Path] = set()
    total = 0
    for path, archive_path in _explicit_bundle_artifacts(root, params, caption_only=caption_only):
        size = path.stat().st_size
        total += size
        if total > max_bytes:
            raise DirectorError("export_budget_exceeded", "Export inputs exceed max_bytes", {"max_bytes": max_bytes, "at": archive_path})
        candidates.append((path, archive_path, size))
        selected_paths.add(path)
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.is_symlink() or path.resolve() == destination.resolve() or path.resolve() in selected_paths:
            continue
        relative = path.relative_to(root).as_posix()
        if _protected_export_path(relative):
            continue
        caption_candidate = path.suffix.lower() in {".vtt", ".srt"} or (
            path.suffix.lower() in {".txt", ".json"} and any(token in path.stem.lower() for token in ("caption", "transcript", "narration", "cue"))
        )
        if _matches(relative, includes) and not _matches(relative, excludes) and (not caption_only or caption_candidate):
            size = path.stat().st_size
            total += size
            if total > max_bytes:
                raise DirectorError("export_budget_exceeded", "Export inputs exceed max_bytes", {"max_bytes": max_bytes, "at": relative})
            candidates.append((path, relative, size))
    if not candidates:
        if caption_only and params.get("cues"):
            candidates = []
        else:
            raise DirectorError("export_empty", "No files matched the export include rules")
    destination.parent.mkdir(parents=True, exist_ok=True)
    compression = zipfile.ZIP_DEFLATED
    manifest: dict[str, Any] = {
        "version": 1, "project": root.name,
        "files": [{"path": relative, "bytes": size} for _, relative, size in candidates],
        "uncompressed_bytes": total,
    }
    try:
        with zipfile.ZipFile(destination, "w", compression=compression, compresslevel=6) as archive:
            for index, (path, relative, _) in enumerate(candidates, 1):
                archive.write(path, relative)
                emit("export_progress", {"completed": index, "total": len(candidates), "path": relative})
            if caption_only:
                from .captions import cues_from_values, format_captions, parse_captions, validate_cues

                caption_source = next((path for path, _, _ in candidates if path.suffix.lower() in {".vtt", ".srt"}), None)
                if params.get("cues") is not None:
                    cues = cues_from_values(params["cues"])
                elif caption_source:
                    cues = parse_captions(caption_source.read_text(encoding="utf-8"))
                else:
                    raise DirectorError("captions_required", "Caption export requires VTT/SRT input or inline cues")
                if not cues:
                    raise DirectorError("captions_required", "Caption export requires at least one timed cue")
                validation = validate_cues(cues)
                if not validation["valid"]:
                    raise DirectorError("invalid_captions", "Caption cues overlap or have invalid timing", validation)
                archive.writestr("captions/captions.vtt", format_captions(cues, "vtt"))
                archive.writestr("captions/captions.srt", format_captions(cues, "srt"))
                manifest["captions"] = {"cue_count": len(cues), "duration": validation["duration"], "sidecars": ["captions/captions.vtt", "captions/captions.srt"]}
            archive.writestr("manim-director-export.json", json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    except DirectorError:
        if destination.exists():
            destination.unlink()
        raise
    except OSError as exc:
        if destination.exists():
            try:
                destination.unlink()
            except OSError:
                pass
        raise DirectorError("export_failed", f"Could not create export bundle: {destination}", {"error": str(exc)}) from exc
    return {
        "path": str(destination), "bytes": destination.stat().st_size,
        "format": "captions" if caption_only else "zip",
        "uncompressed_bytes": total, "files": len(candidates) + (2 if caption_only else 0), "manifest": manifest,
    }


def _exact_project_paths(root: Path, params: Mapping[str, Any]) -> list[Path]:
    raw_values: list[Any] = []
    if params.get("source") is not None:
        raw_values.append(params["source"])
    for key in ("artifacts", "include"):
        value = params.get(key, [])
        if isinstance(value, (str, Path)):
            raw_values.append(value)
        elif isinstance(value, Sequence):
            raw_values.extend(value)
    paths: list[Path] = []
    for raw in raw_values:
        if not isinstance(raw, (str, Path)) or any(token in str(raw) for token in ("*", "?", "[")):
            continue
        try:
            path = confined_path(root, str(raw), must_exist=True)
        except DirectorError:
            continue
        if path.is_file() and path not in paths:
            paths.append(path)
    return paths


def _media_source(root: Path, params: Mapping[str, Any]) -> Path:
    compatible = {".mp4", ".mov", ".webm", ".gif", ".mkv", ".avi", ".m4v"}
    paths = _exact_project_paths(root, params)
    source = next((path for path in paths if path.suffix.lower() in compatible), None)
    if not source:
        raise DirectorError("media_source_required", "Media export requires a project-contained source video", {"candidates": [str(path) for path in paths]})
    return source


def _gif_frame_timing(requested_fps: int) -> tuple[int, int, float]:
    requested = max(1, min(60, int(requested_fps)))
    # GIF delays are integer centiseconds. Select the nearest representable
    # cadence and declare it so artifact validation checks the real contract.
    delay_centiseconds = max(2, min(100, round(100 / requested)))
    return requested, delay_centiseconds, 100.0 / delay_centiseconds


def _transcode_export(params: Mapping[str, Any], emit: Emit, fmt: str) -> dict[str, Any]:
    root = project_root(params)
    source = _media_source(root, params)
    destination = confined_path(root, str(params.get("output", f"output/{source.stem}.{fmt}")))
    if destination.suffix.lower() != f".{fmt}":
        destination = destination.with_suffix(f".{fmt}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    if source.suffix.lower() == destination.suffix.lower():
        if source.resolve() != destination.resolve():
            shutil.copy2(source, destination)
        return {"path": str(destination), "source": str(source), "format": fmt, "bytes": destination.stat().st_size, "transcoded": False}
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        raise DirectorError("executable_not_found", "FFmpeg is required to transcode media exports")
    command = [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-i", str(source)]
    if fmt == "mp4":
        command += ["-c:v", "libx264", "-preset", str(params.get("preset", "medium")), "-crf", str(int(params.get("crf", 18))), "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]
    elif fmt == "webm":
        command += ["-c:v", "libvpx-vp9", "-crf", str(int(params.get("crf", 30))), "-b:v", "0", "-c:a", "libopus", "-b:a", "128k"]
        if bool(params.get("transparent", False)):
            command += ["-pix_fmt", "yuva420p"]
    else:
        requested_fps, delay_centiseconds, effective_fps = _gif_frame_timing(int(params.get("fps", 15)))
        width = max(64, min(3840, int(params.get("width", 960))))
        filters = f"fps=100/{delay_centiseconds},scale='min({width},iw)':-2:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=diff[p];[s1][p]paletteuse=dither=sierra2_4a"
        command += ["-filter_complex", filters, "-loop", "0"]
    command.append(str(destination))
    result = run_command(command, cwd=root, timeout=float(params.get("timeout", 1800)), emit=emit)
    if result.returncode != 0 or not destination.exists():
        from .diagnostics import diagnose_text

        raise DirectorError("export_transcode_failed", f"FFmpeg could not create the {fmt} export", {"diagnostics": diagnose_text(result.output), "tail": result.output[-8000:]})
    media: dict[str, Any] | None = None
    try:
        from .media import probe_media

        media = probe_media(destination)
    except DirectorError:
        pass
    result = {"path": str(destination), "source": str(source), "format": fmt, "bytes": destination.stat().st_size, "transcoded": True, "media": media}
    if fmt == "gif":
        result.update({
            "requested_fps": requested_fps,
            "frame_delay_centiseconds": delay_centiseconds,
            "effective_fps": round(effective_fps, 8),
        })
    return result


def export_bundle(params: Mapping[str, Any], emit: Emit = noop_emit) -> dict[str, Any]:
    fmt = str(params.get("format", "zip")).lower()
    if fmt in {"zip", "bundle"}:
        return _export_zip(params, emit)
    if fmt in {"mp4", "webm", "gif"}:
        return _transcode_export(params, emit, fmt)
    if fmt == "captions":
        return _export_zip(params, emit, caption_only=True)
    raise DirectorError("invalid_export_format", f"Unsupported export format: {fmt}", {"available": ["bundle", "zip", "mp4", "webm", "gif", "captions"]})
