#!/usr/bin/env python3
"""Validate release versions and generate deterministic SHA-256 manifests."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tomllib


ROOT = Path(__file__).resolve().parents[1]
SEMVER = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")
PLUGIN_NAME = "manim-plugin"
REPOSITORY = "https://github.com/Cuuper22/Manim-plugin"
PLUGIN_MANIFESTS = (".codex-plugin/plugin.json", ".claude-plugin/plugin.json")
MARKETPLACES = (".agents/plugins/marketplace.json", ".claude-plugin/marketplace.json")
RELEASE_TARGETS = {
    "x86_64-unknown-linux-musl": "tar.gz",
    "x86_64-apple-darwin": "tar.gz",
    "aarch64-apple-darwin": "tar.gz",
    "x86_64-pc-windows-msvc": "zip",
}


def _json(path: Path) -> dict[str, object]:
    return json.loads(path.read_text(encoding="utf-8"))


def _toml(path: Path) -> dict[str, object]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def _assigned_version(path: Path, name: str) -> str:
    match = re.search(rf'^{name}\s*=\s*["\']([^"\']+)["\']\s*$', path.read_text(encoding="utf-8"), re.MULTILINE)
    if not match:
        raise ValueError(f"{name} is missing from {path}")
    return match.group(1)


def component_versions(root: Path = ROOT) -> dict[str, str]:
    codex_plugin, claude_plugin = (_json(root / manifest) for manifest in PLUGIN_MANIFESTS)
    cargo = _toml(root / "Cargo.toml")
    cargo_lock = _toml(root / "Cargo.lock")
    runtime = _toml(root / "runtime" / "pyproject.toml")
    workbench = _json(root / "workbench" / "package.json")
    workbench_lock = _json(root / "workbench" / "package-lock.json")
    try:
        versions = {
            "Codex plugin": str(codex_plugin["version"]),
            "Claude Code plugin": str(claude_plugin["version"]),
            "MCP launcher": _assigned_version(root / "scripts" / "mcp_launcher.py", "VERSION"),
            "cargo workspace": str(cargo["workspace"]["package"]["version"]),  # type: ignore[index]
            "Python runtime": str(runtime["project"]["version"]),  # type: ignore[index]
            "Python runtime __version__": _assigned_version(
                root / "runtime" / "src" / "manim_director_runtime" / "__init__.py", "__version__"
            ),
            "workbench": str(workbench["version"]),
            "workbench lock": str(workbench_lock["version"]),
            "workbench lock root": str(workbench_lock["packages"][""]["version"]),  # type: ignore[index]
        }
        for package in cargo_lock["package"]:  # type: ignore[union-attr]
            name = package.get("name", "")
            if str(name).startswith("manim-director-"):
                versions[f"Cargo.lock {name}"] = str(package["version"])
    except (KeyError, TypeError) as exc:
        raise ValueError(f"release version declaration is missing: {exc}") from exc
    return versions


def validate_versions(root: Path = ROOT, tag: str | None = None) -> str:
    versions = component_versions(root)
    unique = set(versions.values())
    if len(unique) != 1:
        detail = ", ".join(f"{name}={version}" for name, version in versions.items())
        raise ValueError(f"release component versions differ: {detail}")
    version = unique.pop()
    if not SEMVER.fullmatch(version):
        raise ValueError(f"release version is not semantic: {version}")
    expected_tag = f"v{version}"
    if tag is not None and tag.strip() != expected_tag:
        raise ValueError(f"release tag {tag!r} must exactly equal {expected_tag!r}")

    for manifest in PLUGIN_MANIFESTS:
        if _json(root / manifest).get("name") != PLUGIN_NAME:
            raise ValueError(f"{manifest} must name the plugin {PLUGIN_NAME!r}")
    for marketplace in MARKETPLACES:
        ref = marketplace_ref(root, marketplace)
        if ref != expected_tag:
            raise ValueError(f"{marketplace} ref {ref!r} must equal immutable release ref {expected_tag!r}")
    notices = (root / "THIRD_PARTY_NOTICES.md").read_text(encoding="utf-8")
    if f"{REPOSITORY}/tree/{expected_tag}\n" not in notices:
        raise ValueError(f"THIRD_PARTY_NOTICES.md must link the {expected_tag} source tree")
    return version


def marketplace_ref(root: Path, marketplace: str) -> str:
    try:
        plugins = _json(root / marketplace)["plugins"]
        entries = [entry for entry in plugins if entry.get("name") == PLUGIN_NAME]  # type: ignore[union-attr]
        if len(entries) != 1:
            raise ValueError(f"{marketplace} must contain exactly one {PLUGIN_NAME} entry")
        return str(entries[0]["source"]["ref"])
    except (KeyError, TypeError) as exc:
        raise ValueError(f"{marketplace} entry for {PLUGIN_NAME} is missing its source ref") from exc


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_checksums(directory: Path, output: Path) -> list[Path]:
    directory = directory.resolve()
    output = output.resolve()
    version = validate_versions()
    expected_names = {
        f"manim-director-v{version}-{target}.{extension}"
        for target, extension in RELEASE_TARGETS.items()
    }
    present_names = {
        path.name
        for path in directory.iterdir()
        if path.name.startswith("manim-director-v") and (path.name.endswith(".tar.gz") or path.name.endswith(".zip"))
    }
    if present_names != expected_names:
        missing = ", ".join(sorted(expected_names - present_names)) or "none"
        unexpected = ", ".join(sorted(present_names - expected_names)) or "none"
        raise ValueError(f"release archive set is incomplete (missing: {missing}; unexpected: {unexpected})")
    assets = sorted(directory / name for name in expected_names)
    invalid = [path.name for path in assets if not path.is_file() or path.is_symlink()]
    if invalid:
        raise ValueError(f"release assets must be regular non-symlink files: {', '.join(invalid)}")
    lines = [f"{sha256_file(path)}  {path.name}" for path in assets]
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.{os.getpid()}.tmp")
    temporary.write_text("\n".join(lines) + "\n", encoding="utf-8")
    temporary.replace(output)
    return assets


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    versions = subparsers.add_parser("versions", help="Assert every release component has one version.")
    versions.add_argument("--tag", help="Release tag, for example v1.0.0.")
    checksums = subparsers.add_parser("checksums", help="Write SHA256SUMS for release archives.")
    checksums.add_argument("--directory", type=Path, default=Path("dist"))
    checksums.add_argument("--output", type=Path, default=Path("dist/SHA256SUMS"))
    args = parser.parse_args()

    try:
        if args.command == "versions":
            print(validate_versions(tag=args.tag))
        else:
            assets = write_checksums(args.directory, args.output)
            print(f"wrote {args.output} for {len(assets)} archives")
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        raise SystemExit(str(exc)) from exc


if __name__ == "__main__":
    main()
