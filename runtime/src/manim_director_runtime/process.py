"""Child processes for media tools.

Children stay in the bridge's process group: the engine owns timeouts and kills the
whole group on cancel, so nothing here waits with a timeout or detaches a session.
"""

from __future__ import annotations

import shutil
import subprocess

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


def version_line(path: str, flag: str) -> str | None:
    try:
        completed = subprocess.run(
            [path, flag], stdin=subprocess.DEVNULL, capture_output=True, text=True, check=False
        )
    except OSError:
        return None
    lines = (completed.stdout or completed.stderr).strip().splitlines()
    return lines[0].strip() if lines else None
