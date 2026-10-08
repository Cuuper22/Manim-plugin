"""Child processes for media tools.

Children stay in the bridge's process group: the engine owns timeouts and kills the
whole group on cancel, so nothing here waits with a timeout or detaches a session.
"""

from __future__ import annotations

import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Callable

from .errors import DirectorError, dependency_missing

_INSTALL_HINTS = {
    "ffmpeg": "Install FFmpeg (it provides ffmpeg and ffprobe) and make sure it is on PATH.",
    "ffprobe": "Install FFmpeg (it provides ffmpeg and ffprobe) and make sure it is on PATH.",
}
STDERR_TAIL_CHARS = 4000


def require(executable: str) -> str:
    path = shutil.which(executable)
    if path is None:
        raise dependency_missing(
            executable, _INSTALL_HINTS.get(executable, f"Install {executable}.")
        )
    return path


def run(tool: str, args: list[str], *, lines: Callable[[str], None] | None = None) -> bytes:
    """Run `tool` with `args`; return stdout or raise `media_error` with the stderr tail.
    `lines` receives stdout line by line while the tool runs (ffmpeg's -progress pipe:1)."""

    command = [require(tool), *args]
    if lines is None:
        completed = subprocess.run(
            command, stdin=subprocess.DEVNULL, capture_output=True, check=False
        )
        code, stdout, stderr = completed.returncode, completed.stdout, completed.stderr
    else:
        # stderr goes to a file: a pipe nobody reads while stdout streams could fill and block.
        with tempfile.TemporaryFile() as errors:
            with subprocess.Popen(
                command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=errors
            ) as child:
                assert child.stdout is not None
                chunks = []
                for raw in child.stdout:
                    chunks.append(raw)
                    lines(raw.decode("utf-8", errors="replace").strip())
            errors.seek(0)
            code, stdout, stderr = child.returncode, b"".join(chunks), errors.read()
    if code != 0:
        tail = stderr.decode("utf-8", errors="replace")[-STDERR_TAIL_CHARS:]
        last_line = next((line for line in reversed(tail.splitlines()) if line.strip()), "")
        raise DirectorError(
            "media_error",
            f"{tool} exited with status {code}: {last_line[:300] or 'no output'}",
            {"tool": tool, "exit_code": code, "stderr_tail": tail},
        )
    return stdout


_VERSION = re.compile(r"\d+(?:\.\d+)+[\w.+-]*")


def version(path: str, flag: str) -> str | None:
    """The first version-like token of the tool's banner ("6.1.1" of "ffmpeg version 6.1.1 ...")."""

    try:
        completed = subprocess.run(
            [path, flag], stdin=subprocess.DEVNULL, capture_output=True, text=True, check=False
        )
    except OSError:
        return None
    lines = (completed.stdout or completed.stderr).strip().splitlines()
    match = _VERSION.search(lines[0]) if lines else None
    return match.group() if match else None


def pip_install(*requirements: str) -> str:
    """The pip command for the interpreter running the runtime: a bare `pip` on PATH usually
    belongs to another environment than the installer's venv."""

    args = [sys.executable, "-m", "pip", "install", *requirements]
    return subprocess.list2cmdline(args) if os.name == "nt" else shlex.join(args)
