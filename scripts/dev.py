#!/usr/bin/env python3
"""Run the engine API and the workbench dev server against one project.

Vite (port 4173) serves the workbench with hot reload and proxies /api to the
engine (port 4177); open the printed link, which carries the engine's sign-in
token.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import threading
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit

ROOT = Path(__file__).resolve().parents[1]
ENGINE_PORT = 4177
WORKBENCH_PORT = 4173


def workbench_link(engine_url: str) -> str:
    """The engine's sign-in link, pointed at Vite instead.

    Vite listens on `localhost`, which can resolve to `::1` only (macOS), so the
    link names `localhost` rather than the engine's `127.0.0.1`.
    """
    return urlunsplit(urlsplit(engine_url)._replace(netloc=f"localhost:{WORKBENCH_PORT}"))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", type=Path, help="a directory with director.yaml (or inside one)")
    args = parser.parse_args()

    env = os.environ.copy()
    env.setdefault("RUST_LOG", "manim_director=info")
    engine = [
        *("cargo", "run", "--locked", "-p", "manim-director-cli", "--bin", "manim-director", "--"),
        *("--project", str(args.project.resolve()), "--json", "serve", "--port", str(ENGINE_PORT)),
    ]
    api = subprocess.Popen(engine, cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True)
    # With --json the engine's first stdout line announces its tokenized URL.
    listening = api.stdout.readline()
    if not listening:
        raise SystemExit(api.wait())
    threading.Thread(target=shutil.copyfileobj, args=(api.stdout, sys.stdout), daemon=True).start()
    ui = subprocess.Popen(["npm", "run", "dev"], cwd=ROOT / "workbench", env=env)

    print(f"Workbench (dev): {workbench_link(json.loads(listening)['url'])}", flush=True)

    def stop(*_: object) -> None:
        for child in (ui, api):
            if child.poll() is None:
                child.terminate()

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    try:
        code = ui.wait()
        if code:
            raise SystemExit(code)
    finally:
        stop()
        api.wait(timeout=10)


if __name__ == "__main__":
    main()
