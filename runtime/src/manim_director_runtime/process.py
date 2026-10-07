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


def run(tool: str, args: list[str]) -> bytes:
    """Run `tool` with `args`; return stdout or raise `media_error` with the stderr tail."""

    completed = subprocess.run(
        [require(tool), *args], stdin=subprocess.DEVNULL, capture_output=True, check=False
    )
    if completed.returncode != 0:
        tail = completed.stderr.decode("utf-8", errors="replace")[-STDERR_TAIL_CHARS:]
        last_line = next((line for line in reversed(tail.splitlines()) if line.strip()), "")
        raise DirectorError(
            "media_error",
            f"{tool} exited with status {completed.returncode}: {last_line[:300] or 'no output'}",
            {"tool": tool, "exit_code": completed.returncode, "stderr_tail": tail},
        )
    return completed.stdout


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
