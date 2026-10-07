from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import mcp_launcher  # noqa: E402

LAUNCHER = Path(mcp_launcher.__file__).resolve()
SESSION = [
    {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {"protocolVersion": "2025-06-18"},
    },
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
    {
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {"name": "setup", "arguments": {}},
    },
    {"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "render"}},
    {"jsonrpc": "2.0", "id": 5, "method": "resources/list"},
    {"jsonrpc": "2.0", "id": 6, "method": "ping"},
]


class LauncherTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.home = self.root / "home"
        self.data = self.root / "plugin-data"
        self.path_dir = self.root / "path-bin"
        for directory in (self.home, self.data, self.path_dir):
            directory.mkdir()
        self.env = {
            "HOME": str(self.home),
            "PATH": str(self.path_dir),
            "CLAUDE_PLUGIN_DATA": str(self.data),
        }

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def launch(
        self, messages: list[dict], extra_input: bytes = b""
    ) -> tuple[list[dict], subprocess.CompletedProcess]:
        stdin = b"".join(json.dumps(message).encode() + b"\n" for message in messages) + extra_input
        process = subprocess.run(
            [sys.executable, str(LAUNCHER)],
            input=stdin,
            capture_output=True,
            env=self.env,
            timeout=30,
        )
        return [json.loads(line) for line in process.stdout.splitlines()], process

    def fake_engine(self, directory: Path) -> Path:
        """An executable that prints the argv it was started with."""
        directory.mkdir(parents=True, exist_ok=True)
        engine = directory / "manim-director"
        engine.write_text(
            f"#!{sys.executable}\nimport json, sys\nprint(json.dumps(sys.argv[1:]))\n"
        )
        engine.chmod(0o755)
        return engine


class SetupServerTests(LauncherTestCase):
    def test_missing_engine_serves_only_the_setup_tool(self) -> None:
        responses, process = self.launch(SESSION, extra_input=b"{not json\n[]\n")
        self.assertEqual(process.returncode, 0, process.stderr)
        by_id = {response["id"]: response for response in responses}
        self.assertEqual(len(responses), 8, "the notification must not be answered")

        initialized = by_id[1]["result"]
        self.assertEqual(initialized["protocolVersion"], "2025-06-18")
        self.assertEqual(initialized["capabilities"], {"tools": {"listChanged": False}})
        self.assertEqual(initialized["serverInfo"]["version"], mcp_launcher.VERSION)

        (tool,) = by_id[2]["result"]["tools"]
        self.assertEqual(tool["name"], "setup")
        self.assertEqual(tool["inputSchema"], {"type": "object", "properties": {}})

        setup = by_id[3]["result"]
        self.assertIs(setup["isError"], False)
        text = setup["content"][0]["text"]
        self.assertIn(f"{sys.executable} {mcp_launcher.INSTALLER} --with-manim", text)
        for searched in (
            "PATH",
            self.home / ".local/bin/manim-director",
            self.data / "bin/manim-director",
        ):
            self.assertIn(f"    {searched}", text)

        self.assertEqual(by_id[4]["error"]["code"], -32602)
        self.assertEqual(by_id[5]["error"]["code"], -32601)
        self.assertEqual(by_id[6]["result"], {})
        malformed = [response for response in responses if response["id"] is None]
        self.assertEqual([response["error"]["code"] for response in malformed], [-32700, -32600])

    def test_installer_prefix_from_environment_is_searched_first(self) -> None:
        self.env["MANIM_DIRECTOR_PREFIX"] = str(self.root / "prefix")
        responses, _ = self.launch(SESSION[2:4])
        text = responses[1]["result"]["content"][0]["text"]
        searched = text.split("Searched:\n", 1)[1].split()
        self.assertEqual(searched[:2], ["PATH", str(self.root / "prefix/bin/manim-director")])

    def test_old_python_is_told_what_the_installer_needs(self) -> None:
        with mock.patch.object(mcp_launcher, "installer_python", return_value=None):
            text = mcp_launcher.setup_text(["PATH"])
        self.assertIn("needs Python 3.11 or newer", text)
        self.assertIn(f"{mcp_launcher.INSTALLER} --with-manim", text)


@unittest.skipIf(os.name == "nt", "exec and shebang engines are POSIX behavior")
class EngineExecTests(LauncherTestCase):
    def test_execs_engine_from_installer_prefix_with_claude_project(self) -> None:
        self.fake_engine(self.home / ".local/bin")
        self.env["CLAUDE_PROJECT_DIR"] = str(self.root / "project")
        process = subprocess.run(
            [sys.executable, str(LAUNCHER)],
            capture_output=True,
            env=self.env,
            timeout=30,
            check=True,
        )
        self.assertEqual(
            json.loads(process.stdout), ["mcp", "--project", str(self.root / "project")]
        )

    def test_engine_on_path_wins_and_gets_no_project_flag_outside_claude(self) -> None:
        self.fake_engine(self.home / ".local/bin").write_text("#!/bin/sh\necho wrong\n")
        self.fake_engine(self.path_dir)
        process = subprocess.run(
            [sys.executable, str(LAUNCHER)],
            capture_output=True,
            env=self.env,
            timeout=30,
            check=True,
        )
        self.assertEqual(json.loads(process.stdout), ["mcp"])

    def test_engine_that_cannot_start_falls_back_to_setup(self) -> None:
        broken = self.data / "bin/manim-director"
        broken.parent.mkdir()
        broken.write_bytes(b"\x00not an executable format")
        broken.chmod(0o755)
        responses, process = self.launch(SESSION[3:4])
        self.assertEqual(process.returncode, 0, process.stderr)
        text = responses[0]["result"]["content"][0]["text"]
        self.assertTrue(text.startswith(f"{broken} was found but could not start"), text)


if __name__ == "__main__":
    unittest.main()
