from __future__ import annotations

import os
import queue
import signal
import shutil
import subprocess
import sys
import threading
import time
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable, Mapping, Sequence

from .errors import DirectorError

Emit = Callable[[str, dict[str, Any]], None]


def noop_emit(_event: str, _data: dict[str, Any]) -> None:
    return None


def project_root(params: Mapping[str, Any]) -> Path:
    raw = params.get("project_root", ".")
    root = Path(str(raw)).expanduser().resolve()
    if not root.exists():
        raise DirectorError("project_not_found", f"Project root does not exist: {root}")
    if not root.is_dir():
        raise DirectorError("invalid_project_root", f"Project root is not a directory: {root}")
    return root


def confined_path(root: Path, raw: str | os.PathLike[str], *, must_exist: bool = False) -> Path:
    candidate = Path(raw).expanduser()
    if not candidate.is_absolute():
        candidate = root / candidate
    candidate = candidate.resolve()
    try:
        candidate.relative_to(root.resolve())
    except ValueError as exc:
        raise DirectorError(
            "path_outside_project",
            f"Path is outside the approved project root: {candidate}",
            {"project_root": str(root), "path": str(candidate)},
        ) from exc
    if must_exist and not candidate.exists():
        raise DirectorError("path_not_found", f"Path does not exist: {candidate}")
    return candidate


def atomic_write(path: Path, content: str | bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    if isinstance(content, bytes):
        temp.write_bytes(content)
    else:
        temp.write_text(content, encoding="utf-8")
    temp.replace(path)


def jsonable(value: Any) -> Any:
    if isinstance(value, Path):
        return str(value)
    if hasattr(value, "as_dict"):
        return value.as_dict()
    if isinstance(value, dict):
        return {str(k): jsonable(v) for k, v in value.items()}
    if isinstance(value, (list, tuple, set)):
        return [jsonable(v) for v in value]
    return value


@dataclass(slots=True)
class CommandResult:
    command: list[str]
    returncode: int
    output: str
    elapsed_seconds: float
    output_truncated: bool = False

    def as_dict(self) -> dict[str, Any]:
        return {
            "command": self.command,
            "returncode": self.returncode,
            "output": self.output,
            "output_truncated": self.output_truncated,
            "elapsed_seconds": round(self.elapsed_seconds, 4),
        }


class _TerminationSignal(BaseException):
    def __init__(self, signum: int) -> None:
        self.signum = signum


def _posix_group_exists(process_group: int) -> bool:
    try:
        os.killpg(process_group, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def _windows_taskkill(pid: int, *, force: bool) -> None:
    command = ["taskkill", "/PID", str(pid), "/T"]
    if force:
        command.append("/F")
    try:
        subprocess.run(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=3,
            check=False,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
    except (OSError, subprocess.TimeoutExpired):
        return


def terminate_process_tree(process: subprocess.Popen[str], *, grace_seconds: float = 2.0) -> dict[str, Any]:
    """Terminate a child and all descendants created in its isolated process group."""

    grace_seconds = max(0.0, min(float(grace_seconds), 30.0))
    pid = process.pid
    forced = False
    if os.name == "nt":
        if process.poll() is None:
            try:
                process.send_signal(signal.CTRL_BREAK_EVENT)
            except (AttributeError, OSError, ValueError):
                pass
            # Non-forced taskkill walks the tree while the root PID still identifies it.
            _windows_taskkill(pid, force=False)
            try:
                process.wait(timeout=grace_seconds)
            except subprocess.TimeoutExpired:
                forced = True
                _windows_taskkill(pid, force=True)
                try:
                    process.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=1)
        return {"pid": pid, "process_group": pid, "forced": forced, "platform": "windows"}

    process_group = pid  # start_new_session=True makes the child PID its process-group ID.
    try:
        os.killpg(process_group, signal.SIGTERM)
    except ProcessLookupError:
        pass
    deadline = time.monotonic() + grace_seconds
    while _posix_group_exists(process_group) and time.monotonic() < deadline:
        process.poll()  # Reap the group leader so it does not keep an empty group observable.
        time.sleep(0.02)
    if _posix_group_exists(process_group):
        forced = True
        try:
            os.killpg(process_group, signal.SIGKILL)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=1)
    except subprocess.TimeoutExpired:
        forced = True
        process.kill()
        process.wait(timeout=1)
    return {"pid": pid, "process_group": process_group, "forced": forced, "platform": "posix"}


def _install_termination_handlers() -> tuple[dict[int, Any], dict[str, Any]]:
    previous: dict[int, Any] = {}
    state: dict[str, Any] = {"armed": False, "pending": None}

    def handler(signum: int, _frame: Any) -> None:
        if state["armed"]:
            raise _TerminationSignal(signum)
        state["pending"] = signum

    if threading.current_thread() is not threading.main_thread():
        return previous, state
    candidates = [signal.SIGTERM, signal.SIGINT]
    if hasattr(signal, "SIGBREAK"):
        candidates.append(signal.SIGBREAK)
    for candidate in candidates:
        try:
            previous[candidate] = signal.getsignal(candidate)
            signal.signal(candidate, handler)
        except (OSError, ValueError):
            continue
    return previous, state


def _set_handlers(previous: Mapping[int, Any], value: Any) -> None:
    if threading.current_thread() is not threading.main_thread():
        return
    for candidate in previous:
        try:
            signal.signal(candidate, value)
        except (OSError, ValueError):
            continue


def _restore_handlers(previous: Mapping[int, Any]) -> None:
    if threading.current_thread() is not threading.main_thread():
        return
    for candidate, handler in previous.items():
        try:
            signal.signal(candidate, handler)
        except (OSError, ValueError):
            continue


def run_command(
    command: Sequence[str],
    *,
    cwd: Path | None = None,
    timeout: float = 900,
    emit: Emit = noop_emit,
    env: Mapping[str, str] | None = None,
    check: bool = False,
    max_output_chars: int = 4_000_000,
    termination_grace_seconds: float = 2.0,
) -> CommandResult:
    """Run a bounded child process and stream merged output as log events."""

    if timeout <= 0:
        raise DirectorError("invalid_timeout", "Command timeout must be positive")
    if max_output_chars < 1:
        raise DirectorError("invalid_output_limit", "max_output_chars must be positive")
    started = time.monotonic()
    merged_env = os.environ.copy()
    if env:
        merged_env.update({str(k): str(v) for k, v in env.items()})
    emit("command_started", {"command": list(command), "cwd": str(cwd) if cwd else None})
    previous_handlers, termination_state = _install_termination_handlers()
    popen_options: dict[str, Any] = {}
    if os.name == "nt":
        popen_options["creationflags"] = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
    else:
        popen_options["start_new_session"] = True
    try:
        process = subprocess.Popen(
            list(command),
            cwd=str(cwd) if cwd else None,
            env=merged_env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
            **popen_options,
        )
    except FileNotFoundError as exc:
        _restore_handlers(previous_handlers)
        raise DirectorError("executable_not_found", f"Executable not found: {command[0]}") from exc
    except BaseException:
        _restore_handlers(previous_handlers)
        raise
    termination_state["armed"] = True
    if termination_state["pending"] is not None:
        _set_handlers(previous_handlers, signal.SIG_IGN)
        terminate_process_tree(process, grace_seconds=termination_grace_seconds)
        if process.stdout is not None:
            process.stdout.close()
        _restore_handlers(previous_handlers)
        raise SystemExit(128 + int(termination_state["pending"]))
    lines: deque[str] = deque()
    buffered_chars = 0
    output_truncated = False
    assert process.stdout is not None
    output_queue: queue.Queue[str | None] = queue.Queue()

    def read_output() -> None:
        assert process.stdout is not None
        try:
            for line in iter(process.stdout.readline, ""):
                output_queue.put(line)
        except (OSError, ValueError):
            pass
        finally:
            output_queue.put(None)

    reader = threading.Thread(target=read_output, name="director-command-output", daemon=True)
    reader.start()
    try:
        stream_closed = False
        while True:
            if time.monotonic() - started > timeout:
                raise DirectorError(
                    "command_timeout",
                    f"Command exceeded {timeout:g} seconds",
                    {"command": list(command), "output": "".join(lines)[-8000:]},
                )
            try:
                line = output_queue.get(timeout=0.05)
            except queue.Empty:
                line = ""
            if line is None:
                stream_closed = True
            elif line:
                lines.append(line)
                buffered_chars += len(line)
                while buffered_chars > max_output_chars and lines:
                    buffered_chars -= len(lines.popleft())
                    output_truncated = True
                clean_line = line.rstrip("\r\n")
                emit("log", {"stream": "combined", "message": clean_line[-8000:], "truncated": len(clean_line) > 8000})
                continue
            if process.poll() is not None and stream_closed:
                break
        returncode = process.wait()
    except DirectorError as exc:
        _set_handlers(previous_handlers, signal.SIG_IGN)
        exc.data.setdefault("cleanup", terminate_process_tree(process, grace_seconds=termination_grace_seconds))
        raise
    except _TerminationSignal as interrupted:
        # Ignore repeated termination signals during the bounded cleanup window.
        _set_handlers(previous_handlers, signal.SIG_IGN)
        terminate_process_tree(process, grace_seconds=termination_grace_seconds)
        raise SystemExit(128 + interrupted.signum) from None
    except BaseException:
        _set_handlers(previous_handlers, signal.SIG_IGN)
        terminate_process_tree(process, grace_seconds=termination_grace_seconds)
        raise
    finally:
        _restore_handlers(previous_handlers)
        try:
            process.stdout.close()
        except OSError:
            pass
        reader.join(timeout=1)
    elapsed = time.monotonic() - started
    result = CommandResult(list(command), returncode, "".join(lines), elapsed, output_truncated)
    emit("command_finished", {"returncode": returncode, "elapsed_seconds": round(elapsed, 4)})
    if check and returncode != 0:
        from .diagnostics import diagnose_text

        raise DirectorError(
            "command_failed",
            f"Command failed with exit code {returncode}",
            {"command": list(command), "diagnostics": diagnose_text(result.output), "tail": result.output[-8000:]},
        )
    return result


def executable_version(executable: str, args: Iterable[str] = ("--version",), timeout: float = 5) -> dict[str, Any]:
    resolved = shutil.which(executable)
    if not resolved:
        return {"available": False, "path": None, "version": None}
    try:
        completed = subprocess.run(
            [resolved, *args], capture_output=True, text=True, timeout=timeout, check=False
        )
        text = (completed.stdout or completed.stderr).strip().splitlines()
        return {
            "available": completed.returncode == 0,
            "path": resolved,
            "version": text[0] if text else None,
            "returncode": completed.returncode,
        }
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"available": False, "path": resolved, "version": None, "error": str(exc)}


def python_executable() -> str:
    return sys.executable or "python"
