from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import install  # noqa: E402
import package_release  # noqa: E402
import third_party_licenses as licenses  # noqa: E402

MIT = "MIT License\n\nCopyright (c) {}\n\nPermission is hereby granted..."
APACHE = "Apache License\nVersion 2.0, January 2004\n\nTERMS AND CONDITIONS"


def crate(id_: str, deps: list[tuple[str, str | None]]) -> tuple[dict, dict]:
    package = {
        "id": id_,
        "name": id_,
        "version": "1.0.0",
        "license": "MIT OR Apache-2.0",
        "manifest_path": f"/registry/{id_}/Cargo.toml",
    }
    node = {"id": id_, "deps": [{"pkg": pkg, "dep_kinds": [{"kind": kind}]} for pkg, kind in deps]}
    return package, node


class RustTests(unittest.TestCase):
    def test_only_what_the_engine_links_is_listed(self) -> None:
        graph = [
            crate("manim-director-cli", [("manim-director-core", None), ("clap", None)]),
            crate("manim-director-core", [("serde", None), ("tempfile", "dev")]),
            crate("clap", [("serde", None), ("cc", "build")]),
            crate("serde", []),
            crate("tempfile", []),
            crate("cc", []),
            crate("unused", []),
        ]
        metadata = {
            "packages": [package for package, _ in graph],
            "resolve": {"nodes": [node for _, node in graph]},
            "workspace_members": ["manim-director-cli", "manim-director-core"],
        }
        listed = licenses.rust_packages(metadata)
        self.assertEqual(sorted(package.name for package in listed), ["clap", "serde"])
        self.assertEqual(listed[0].directory.parent, Path("/registry"))


class NpmTests(unittest.TestCase):
    def test_runtime_closure_follows_node_resolution_and_bundled_vite(self) -> None:
        lock = {
            "packages": {
                "": {"dependencies": {"react-dom": "^19"}, "devDependencies": {"vite": "^8"}},
                "node_modules/react-dom": {
                    "version": "19.0.0",
                    "license": "MIT",
                    "dependencies": {"scheduler": "^0.27"},
                    "peerDependencies": {"react": "^19", "@types/react": "*"},
                    "peerDependenciesMeta": {"@types/react": {"optional": True}},
                },
                "node_modules/react-dom/node_modules/scheduler": {"version": "0.27.0"},
                "node_modules/scheduler": {"version": "0.1.0"},
                "node_modules/react": {"version": "19.0.0", "license": "MIT"},
                "node_modules/vite": {"version": "8.0.0", "dependencies": {"rolldown": "*"}},
                "node_modules/rolldown": {"version": "1.0.0"},
            }
        }
        with tempfile.TemporaryDirectory() as raw:
            workbench = Path(raw)
            (workbench / "package-lock.json").write_text(json.dumps(lock))
            listed = {
                (package.name, package.version): package.directory.relative_to(workbench)
                for package in licenses.npm_packages(workbench)
            }
        self.assertEqual(
            listed,
            {
                ("react-dom", "19.0.0"): Path("node_modules/react-dom"),
                ("scheduler", "0.27.0"): Path("node_modules/react-dom/node_modules/scheduler"),
                ("react", "19.0.0"): Path("node_modules/react"),
                ("vite", "8.0.0"): Path("node_modules/vite"),
            },
        )


class RenderTests(unittest.TestCase):
    def package(self, root: Path, name: str, files: dict[str, str]) -> licenses.Package:
        directory = root / name
        directory.mkdir()
        for filename, text in files.items():
            (directory / filename).write_text(text)
        (directory / "README.md").write_text("not a license")
        return licenses.Package("Rust", name, "1.0.0", "MIT OR Apache-2.0", directory)

    def test_each_text_appears_once_under_every_package_that_ships_it(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            packages = [
                self.package(root, "b", {"LICENSE-MIT": MIT.format("B"), "LICENSE-APACHE": APACHE}),
                self.package(root, "a", {"COPYING": MIT.format("A"), "LICENSE-APACHE": APACHE}),
                # Re-wrapped, still the same license.
                self.package(root, "c", {"LICENSE.txt": APACHE.replace("\n", "\n\n")}),
            ]
            text = licenses.render("x86_64-unknown-linux-musl", packages)
        self.assertEqual(text.count("Apache License"), 1)
        self.assertIn(
            "Rust: a 1.0.0 (MIT OR Apache-2.0), LICENSE-APACHE\n"
            "Rust: b 1.0.0 (MIT OR Apache-2.0), LICENSE-APACHE\n"
            "Rust: c 1.0.0 (MIT OR Apache-2.0), LICENSE.txt\n",
            text,
        )
        self.assertIn("Copyright (c) A", text)
        self.assertIn("Copyright (c) B", text)
        self.assertNotIn("not a license", text)

    def test_a_package_without_a_license_file_stops_the_release(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            bare = self.package(Path(raw), "bare", {})
            with self.assertRaisesRegex(ValueError, "bare 1.0.0 ships no license file"):
                licenses.render("x86_64-unknown-linux-musl", [bare])


class ReleaseArchiveTests(unittest.TestCase):
    def test_archives_carry_exactly_the_notices_the_installer_expects(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            binary = Path(raw) / "manim-director"
            binary.write_bytes(b"engine")
            members = package_release.release_members(binary, binary.name, "licenses")
            too_long = "x" * (install.MAX_NOTICE_BYTES + 1)
            with self.assertRaisesRegex(ValueError, "as install.py expects"):
                package_release.release_members(binary, binary.name, too_long)
        names = {name for name, _, _ in members}
        self.assertEqual(names, install.NOTICE_MEMBERS | {binary.name})
        self.assertIn((package_release.LICENSES_MEMBER, b"licenses", 0o644), members)


if __name__ == "__main__":
    unittest.main()
