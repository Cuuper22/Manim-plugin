from __future__ import annotations

import argparse
import json
import sys

from .protocol import handle_request, serve


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="manim-director-runtime", description="Manim Director JSONL runtime bridge")
    subparsers = parser.add_subparsers(dest="command")
    subparsers.add_parser("bridge", help="Read JSONL requests from standard input")
    call_parser = subparsers.add_parser("call", help="Run one bridge method and print its JSON messages")
    call_parser.add_argument("method")
    call_parser.add_argument("--params", default="{}", help="JSON object passed as params")
    ingest_parser = subparsers.add_parser("ingest", help="Copy and summarize local sources into a project manifest")
    ingest_parser.add_argument("paths", nargs="+")
    ingest_parser.add_argument("--project-root", default=".")
    ingest_parser.add_argument("--destination-dir", default="sources")
    ingest_parser.add_argument("--manifest", default="sources/manifest.json")
    ingest_parser.add_argument("--summary-chars", type=int, default=2000)
    args = parser.parse_args(argv)
    if args.command in {None, "bridge"}:
        return serve()
    method = args.command
    if args.command == "call":
        method = args.method
        try:
            params = json.loads(args.params)
        except json.JSONDecodeError as exc:
            parser.error(f"--params is not valid JSON: {exc}")
        if not isinstance(params, dict):
            parser.error("--params must decode to a JSON object")
    else:
        params = {
            "project_root": args.project_root, "paths": args.paths,
            "destination_dir": args.destination_dir, "manifest": args.manifest,
            "summary_chars": args.summary_chars,
        }

    def write(message: dict) -> None:
        print(json.dumps(message, ensure_ascii=False, separators=(",", ":")))

    handle_request({"request_id": "cli", "method": method, "params": params}, write)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
