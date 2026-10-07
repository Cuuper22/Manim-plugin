#!/usr/bin/env python3
"""Start Manim Director's MCP server, or explain how to install the engine.

The plugin's `.mcp.json` runs this script with the host's `python3`, so a missing
engine becomes a `setup` tool instead of a server that fails to start. When the
engine is installed the script execs `manim-director mcp`; otherwise it answers
MCP over stdio with that single tool. It uses only the standard library and
stays compatible with Python 3.8 (the system python3 on some macOS versions).
"""

from __future__ import annotations

import json
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path
from typing import BinaryIO, Mapping

VERSION = "2.0.0"
PROTOCOL_VERSION = "2025-06-18"
ENGINE = "manim-director"
INSTALLER = Path(__file__).resolve().with_name("install.py")
MIN_PYTHON = (3, 11)
SETUP_TOOL = {
    "name": "setup",
    "description": (
        "The Manim Director engine is not installed or cannot start. Returns the command "
        "that installs it and the paths that were searched."
    ),
    "inputSchema": {"type": "object", "properties": {}},
}


def install_prefixes(env: Mapping[str, str]) -> list[Path]:
    """Where scripts/install.py may have put the engine, besides PATH."""
    prefixes = []
    if env.get("MANIM_DIRECTOR_PREFIX"):
        prefixes.append(Path(env["MANIM_DIRECTOR_PREFIX"]).expanduser())
    prefixes.append(Path.home() / ".local")
    if env.get("CLAUDE_PLUGIN_DATA"):
        prefixes.append(Path(env["CLAUDE_PLUGIN_DATA"]))
    return prefixes


def find_engine(env: Mapping[str, str]) -> tuple[str | None, list[str]]:
    """Return the engine executable, if any, and every location searched."""
    searched = ["PATH"]
    on_path = shutil.which(ENGINE, path=env.get("PATH", os.defpath))
    if on_path:
        return on_path, searched
    name = ENGINE + (".exe" if os.name == "nt" else "")
    for prefix in install_prefixes(env):
        candidate = prefix / "bin" / name
        searched.append(str(candidate))
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate), searched
    return None, searched


def run_engine(engine: str, env: Mapping[str, str]) -> None:
    argv = [engine, "mcp"]
    # Claude Code names the project root; other hosts start the server inside it.
    if env.get("CLAUDE_PROJECT_DIR"):
        argv += ["--project", env["CLAUDE_PROJECT_DIR"]]
    if os.name == "nt":
        # Windows has no exec: run the engine as a child that inherits stdio.
        sys.exit(subprocess.call(argv))
    os.execv(engine, argv)


def installer_python() -> str | None:
    """An interpreter new enough for scripts/install.py, preferring this one."""
    if sys.version_info >= MIN_PYTHON:
        return sys.executable
    for minor in range(20, MIN_PYTHON[1] - 1, -1):
        found = shutil.which(f"python3.{minor}")
        if found:
            return found
    return None


def setup_text(searched: list[str], problem: str | None = None) -> str:
    python = installer_python()
    lines = [problem or "Manim Director's engine (manim-director) is not installed.", ""]
    if python is None:
        found = ".".join(map(str, sys.version_info[:3]))
        lines += [
            f"The installer needs Python 3.11 or newer; {sys.executable} is {found}.",
            "Install a newer Python, then run its interpreter on:",
            f"    {shlex.quote(str(INSTALLER))} --with-manim",
        ]
    else:
        lines += [
            "Install the engine and its Manim runtime (into ~/.local by default):",
            f"    {shlex.join([python, str(INSTALLER), '--with-manim'])}",
        ]
    lines += [
        "",
        "Then reconnect this MCP server (or start a new session) and call its",
        "doctor tool to check Manim, LaTeX and FFmpeg.",
        "",
        "Searched:",
        *(f"    {location}" for location in searched),
    ]
    return "\n".join(lines)


def answer(message: dict, text: str) -> dict | None:
    """The response to one JSON-RPC message; notifications get none."""
    if "id" not in message:
        return None
    request_id = message["id"]
    method = message.get("method")
    params = message.get("params")
    params = params if isinstance(params, dict) else {}
    if method == "initialize":
        result = {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {"listChanged": False}},
            "serverInfo": {"name": "manim-director", "version": VERSION},
            "instructions": "The engine is not installed. Call setup for the install command.",
        }
    elif method == "ping":
        result = {}
    elif method == "tools/list":
        result = {"tools": [SETUP_TOOL]}
    elif method == "tools/call":
        if params.get("name") != "setup":
            return rpc_error(request_id, -32602, f"unknown tool: {params.get('name')}")
        result = {"content": [{"type": "text", "text": text}], "isError": False}
    else:
        return rpc_error(request_id, -32601, f"method not found: {method}")
    return {"jsonrpc": "2.0", "id": request_id, "result": result}


def rpc_error(request_id: object, code: int, message: str) -> dict:
    return {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}


def serve_setup(text: str, incoming: BinaryIO, outgoing: BinaryIO) -> None:
    """Answer newline-delimited JSON-RPC until the client closes stdin."""
    for line in incoming:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
        except ValueError as error:
            response = rpc_error(None, -32700, f"parse error: {error}")
        else:
            if isinstance(message, dict):
                response = answer(message, text)
            else:
                response = rpc_error(None, -32600, "invalid request: expected an object")
        if response is not None:
            outgoing.write(json.dumps(response).encode("ascii") + b"\n")
            outgoing.flush()


def main() -> None:
    engine, searched = find_engine(os.environ)
    problem = None
    if engine:
        try:
            run_engine(engine, os.environ)
        except OSError as error:
            problem = f"{engine} was found but could not start: {error}. Reinstall it."
    serve_setup(setup_text(searched, problem), sys.stdin.buffer, sys.stdout.buffer)


if __name__ == "__main__":
    main()
