"""Entry point: `python -P -m manim_director_runtime bridge [--preload]`."""

import os

# Before anything can write to stdout: frames go to a private copy of fd 1, and fd 1
# itself becomes stderr, so print(), Manim's console and C libraries never corrupt them.
_PROTOCOL_FD = os.dup(1)
os.dup2(2, 1)

import argparse  # noqa: E402
import sys  # noqa: E402

from .protocol import run_bridge  # noqa: E402


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(prog="python -m manim_director_runtime")
    commands = parser.add_subparsers(dest="command", required=True)
    bridge = commands.add_parser("bridge", help="serve one request over the JSONL bridge")
    bridge.add_argument(
        "--preload", action="store_true", help="import Manim before signalling ready"
    )
    args = parser.parse_args(argv)
    run_bridge(_PROTOCOL_FD, preload=args.preload, stdin=sys.stdin.buffer)


if __name__ == "__main__":
    main()
