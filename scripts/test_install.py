from __future__ import annotations

import os
import sys
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import install  # noqa: E402


class InstallExecutableTests(unittest.TestCase):
    def test_replaces_by_rename_and_marks_executable(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            directory = Path(raw_tmp)
            source = directory / "built"
            source.write_bytes(b"new engine")
            source.chmod(0o644)
            destination = directory / "bin" / "manim-director"
            destination.parent.mkdir()
            destination.write_bytes(b"old engine")
            # A running engine keeps the old inode; a rename leaves it untouched.
            with destination.open("rb") as running:
                install.install_executable(source, destination)
                self.assertEqual(running.read(), b"old engine")
            self.assertEqual(destination.read_bytes(), b"new engine")
            if os.name != "nt":
                self.assertEqual(destination.stat().st_mode & 0o111, 0o111)
            self.assertEqual(os.listdir(destination.parent), ["manim-director"])


class FallbackTests(unittest.TestCase):
    def run_main(self, toolchains: str | None) -> tuple[mock.Mock, Path]:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        prefix = Path(tmp.name)
        unreachable = urllib.error.URLError("unreachable")
        with (
            mock.patch.object(sys, "argv", ["install.py", "--prefix", str(prefix)]),
            mock.patch.object(install, "download_binary", side_effect=unreachable),
            mock.patch.object(install.shutil, "which", return_value=toolchains),
            mock.patch.object(install, "build_binary") as build,
            mock.patch.object(install, "install_runtime"),
            mock.patch("builtins.print"),
        ):
            install.main()
        return build, prefix.resolve() / "bin" / install.BINARY_NAME

    def test_failed_download_builds_locally_when_rust_and_node_exist(self) -> None:
        build, installed = self.run_main("/usr/bin/tool")
        build.assert_called_once_with(installed)

    def test_failed_download_without_toolchains_says_how_to_build(self) -> None:
        with self.assertRaisesRegex(SystemExit, "--from-source"):
            self.run_main(None)


if __name__ == "__main__":
    unittest.main()
