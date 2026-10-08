#!/usr/bin/env python3
"""Collect the license texts of the third-party code a release binary contains.

The engine statically links Rust crates and embeds the built workbench. Their
licenses (MIT, Apache-2.0, BSD, ...) require their texts to accompany every
copy, so package_release.py puts this file in each release archive.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ENGINE_PACKAGE = "manim-director-cli"
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice|unlicense)", re.IGNORECASE)
# Vite writes its module-preload helper into the page; its own dependencies are build tools.
BUNDLED_TOOLS = ("vite",)
RULE = "=" * 78


@dataclass(frozen=True)
class Package:
    ecosystem: str
    name: str
    version: str
    license: str
    directory: Path

    @property
    def label(self) -> str:
        return f"{self.name} {self.version}"


def rust_packages(metadata: dict) -> list[Package]:
    """The engine's normal (non-dev, non-build) dependencies, transitively."""
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    workspace = set(metadata["workspace_members"])
    engine = next(id_ for id_ in workspace if packages[id_]["name"] == ENGINE_PACKAGE)
    seen: set[str] = set()
    pending = [engine]
    while pending:
        id_ = pending.pop()
        if id_ in seen:
            continue
        seen.add(id_)
        pending += [
            dep["pkg"]
            for dep in nodes[id_]["deps"]
            if any(kind["kind"] is None for kind in dep["dep_kinds"])
        ]
    return [
        Package(
            "Rust",
            packages[id_]["name"],
            packages[id_]["version"],
            packages[id_]["license"] or "see files",
            Path(packages[id_]["manifest_path"]).parent,
        )
        for id_ in seen - workspace
    ]


def npm_packages(workbench: Path) -> list[Package]:
    """The workbench's runtime dependencies, transitively, as npm installed them."""
    lock = json.loads((workbench / "package-lock.json").read_text(encoding="utf-8"))["packages"]
    root = lock[""]
    pending = [("", name, True) for name in root.get("dependencies", {})]
    pending += [("", name, False) for name in BUNDLED_TOOLS]
    found: dict[str, Package] = {}
    while pending:
        parent, name, walk = pending.pop()
        path = _installed_path(lock, parent, name)
        if path in found:
            continue
        entry = lock[path]
        found[path] = Package(
            "npm", name, entry["version"], entry.get("license", "see files"), workbench / path
        )
        if walk:
            optional = entry.get("peerDependenciesMeta", {})
            peers = [peer for peer in entry.get("peerDependencies", {}) if peer not in optional]
            pending += [(path, dep, True) for dep in [*entry.get("dependencies", {}), *peers]]
    return list(found.values())


def _installed_path(lock: dict, parent: str, name: str) -> str:
    """Where Node resolves `name` from `parent`: the nearest enclosing node_modules."""
    base = parent
    while True:
        candidate = f"{base}/node_modules/{name}" if base else f"node_modules/{name}"
        if candidate in lock:
            return candidate
        if not base:
            raise ValueError(f"{name} (needed by {parent or 'the workbench'}) is not in the lock")
        base = base.rsplit("/node_modules/", 1)[0] if "/node_modules/" in base else ""


def license_files(package: Package) -> tuple[tuple[str, str], ...]:
    files = sorted(
        path
        for path in package.directory.iterdir()
        if path.is_file() and LICENSE_FILE.match(path.name)
    )
    if not files:
        raise ValueError(f"{package.ecosystem} package {package.label} ships no license file")
    return tuple(
        (path.name, path.read_text(encoding="utf-8", errors="replace").strip()) for path in files
    )


def render(target: str, packages: Iterable[Package]) -> str:
    """Each distinct license text once, under the packages that ship it."""
    texts: dict[str, tuple[str, list[str]]] = {}
    for package in sorted(packages, key=lambda item: (item.ecosystem, item.name, item.version)):
        for filename, text in license_files(package):
            # Copies that differ only in line wrapping are the same license.
            _, users = texts.setdefault(" ".join(text.split()), (text, []))
            users.append(f"{package.ecosystem}: {package.label} ({package.license}), {filename}")
    header = (
        f"Third-party licenses for the Manim Director engine ({target})\n\n"
        "The manim-director binary statically links the Rust crates below and embeds a web\n"
        "workbench built from the npm packages below. Each license text appears once, after\n"
        "the packages that ship it.\n"
    )
    sections = (f"{RULE}\n" + "\n".join(users) + f"\n\n{text}\n" for text, users in texts.values())
    return "\n".join([header, *sections])


def collect(target: str, root: Path = ROOT) -> str:
    metadata = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
        cwd=root,
        check=True,
        capture_output=True,
        encoding="utf-8",
    ).stdout
    packages = rust_packages(json.loads(metadata)) + npm_packages(root / "workbench")
    return render(target, packages)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, help="Rust target triple of the release")
    args = parser.parse_args()
    try:
        sys.stdout.buffer.write(collect(args.target).encode())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Could not collect third-party licenses: {error}") from error


if __name__ == "__main__":
    main()
