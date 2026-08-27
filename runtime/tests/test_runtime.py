from __future__ import annotations

import io
import json
import os
import signal
import subprocess
import tempfile
import time
import unittest
import zipfile
import sys
from pathlib import Path
from unittest.mock import patch

from manim_director_runtime.assets import assets
from manim_director_runtime.captions import Cue, format_captions, parse_captions, validate_cues
from manim_director_runtime.diagnostics import diagnose_text
from manim_director_runtime.doctor import _version_args
from manim_director_runtime.errors import DirectorError
from manim_director_runtime.exporting import _gif_frame_timing, export_bundle
from manim_director_runtime.inspection import discover, inspect_file
from manim_director_runtime.ingest import ingest
from manim_director_runtime.math_validation import math_validate, safe_evaluate
from manim_director_runtime.media import compact_media_summary, create_contact_sheet
from manim_director_runtime.protocol import serve
from manim_director_runtime.qa import analyze_images
from manim_director_runtime.rendering import build_render_command, render as render_scene
from manim_director_runtime.sample import generalized_fibonacci_source
from manim_director_runtime.scaffold import scaffold
from manim_director_runtime.templates import GENERATORS
from manim_director_runtime.util import CommandResult, run_command


class RuntimeTests(unittest.TestCase):
    def test_doctor_uses_native_ffmpeg_version_flag(self) -> None:
        self.assertEqual(_version_args("ffmpeg"), ("-version",))
        self.assertEqual(_version_args("ffprobe"), ("-version",))
        self.assertEqual(_version_args("manim"), ("--version",))

    def test_gif_timing_declares_nearest_centisecond_cadence(self) -> None:
        requested, delay, effective = _gif_frame_timing(15)
        self.assertEqual((requested, delay), (15, 7))
        self.assertAlmostEqual(effective, 100 / 7)

    def test_bridge_contract_and_recovery_from_bad_json(self) -> None:
        source = io.StringIO('{bad\n{"request_id":"x","method":"capabilities","params":{}}\n')
        output = io.StringIO()
        self.assertEqual(serve(source, output), 0)
        messages = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(messages[0]["type"], "error")
        self.assertEqual(messages[0]["error"]["code"], "invalid_json")
        self.assertEqual(messages[1]["request_id"], "x")
        self.assertEqual(messages[1]["type"], "result")

    def test_scaffold_sample_and_discovery(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            result = scaffold({"project_root": raw, "name": "Sequence Lab", "sample": True})
            root = Path(raw)
            self.assertTrue((root / "director.yaml").exists())
            director = (root / "director.yaml").read_text(encoding="utf-8")
            self.assertIn("theme:\n  preset: midnight", director)
            self.assertIn(f"  seed: {result['seed']}\n", director)
            self.assertEqual(result["seed"], scaffold_seed := 1465734834)
            with tempfile.TemporaryDirectory() as second_raw:
                second = scaffold({"project_root": second_raw, "name": "Sequence Lab", "sample": True})
            self.assertEqual(second["seed"], scaffold_seed)
            sample = (root / "scenes/main.py").read_text()
            compile(sample, "main.py", "exec")
            report = discover({"project_root": raw})
            self.assertEqual([item["name"] for item in report["scenes"]], ["GeneralizedFibonacciScene"])
            self.assertGreaterEqual(len(report["scenes"][0]["sections"]), 3)
            self.assertEqual(result["sample_scene"], "GeneralizedFibonacciScene")

    def test_scaffold_nonempty_requires_intent_and_preserves_unrelated_files(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            unrelated = root / "notes.txt"
            unrelated.write_text("keep exactly\n", encoding="utf-8")
            before = {path.relative_to(root): path.read_bytes() for path in root.rglob("*") if path.is_file()}
            with self.assertRaises(DirectorError) as raised:
                scaffold({"project_root": raw, "name": "Safe Init"})
            self.assertEqual(raised.exception.code, "project_not_empty")
            after = {path.relative_to(root): path.read_bytes() for path in root.rglob("*") if path.is_file()}
            self.assertEqual(after, before)
            self.assertFalse((root / "director.yaml").exists())

            custom_readme = root / "README.md"
            custom_readme.write_text("# Existing documentation\n", encoding="utf-8")
            merged = scaffold({"project_root": raw, "name": "Safe Init", "sample": False, "merge": True})
            self.assertEqual(unrelated.read_text(encoding="utf-8"), "keep exactly\n")
            self.assertEqual(custom_readme.read_text(encoding="utf-8"), "# Existing documentation\n")
            self.assertIn(str(custom_readme), merged["preserved_files"])
            starter = (root / "scenes/main.py").read_text(encoding="utf-8")
            compile(starter, "main.py", "exec")
            self.assertEqual(discover({"project_root": raw})["scenes"][0]["name"], "MainScene")

    def test_scaffold_force_updates_managed_files_only(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            unrelated = root / "keep.bin"
            unrelated.write_bytes(b"unrelated\x00payload")
            (root / "README.md").write_text("old managed readme\n", encoding="utf-8")
            (root / ".gitignore").write_text("custom-build/\n", encoding="utf-8")
            result = scaffold({"project_root": raw, "name": "Forced Init", "force": True, "seed": 42})
            self.assertEqual(result["seed"], 42)
            self.assertEqual(unrelated.read_bytes(), b"unrelated\x00payload")
            self.assertIn("# Forced Init", (root / "README.md").read_text(encoding="utf-8"))
            ignore = (root / ".gitignore").read_text(encoding="utf-8")
            self.assertIn("custom-build/", ignore)
            self.assertIn(".manim-director/media/", ignore)
            self.assertIn("  seed: 42\n", (root / "director.yaml").read_text(encoding="utf-8"))

    def test_inspection_does_not_execute_source(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = Path(raw) / "scene.py"
            marker = Path(raw) / "executed"
            path.write_text(f'from manim import *\nPath({str(marker)!r}).write_text("bad")\nclass Demo(Scene):\n def construct(self):\n  self.play(Write(Text("x")))\n', encoding="utf-8")
            report = inspect_file(path)
            self.assertTrue(report["valid_python"])
            self.assertEqual(report["scenes"][0]["name"], "Demo")
            self.assertFalse(marker.exists())

    def test_render_command_profiles_sections_and_alpha(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "scenes").mkdir()
            (root / "scenes/main.py").write_text("from manim import *\nclass Demo(Scene):\n def construct(self):\n  self.next_section('proof')\n", encoding="utf-8")
            command, metadata = build_render_command({
                "project_root": raw, "scene": "Demo", "manim_executable": "/bin/echo",
                "renderer": "opengl", "format": "webm", "transparent": True,
                "profile": "custom", "width": 1080, "height": 1920, "fps": 30,
                "sections": ["proof"],
            }, mode="section")
            self.assertIn("--save_sections", command)
            self.assertIn("--transparent", command)
            self.assertIn("1080,1920", command)
            self.assertEqual(metadata["renderer"], "opengl")
            custom_command, custom_metadata = build_render_command({
                "project_root": raw, "scene": "Demo", "manim_executable": "/bin/echo",
                "profile": "vertical", "width": 1080, "height": 1920, "fps": 60,
            })
            self.assertEqual(custom_metadata["profile"].name, "vertical")
            self.assertIn("1080,1920", custom_command)
            with self.assertRaises(DirectorError):
                build_render_command({"project_root": raw, "scene": "Demo", "manim_executable": "/bin/echo", "transparent": True, "format": "mp4"})

    def test_render_result_excludes_partial_segments_and_compacts_probe(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "scenes").mkdir()
            (root / "scenes/main.py").write_text(
                "from manim import *\nclass Demo(Scene):\n def construct(self): self.wait(.1)\n",
                encoding="utf-8",
            )
            media = root / ".manim-director/media/videos/main/480p15"
            final = media / "Demo.mp4"
            partial = media / "partial_movie_files/Demo/129292.mp4"
            unrelated = media / "Unrelated.mp4"

            def fake_run(command, **_kwargs):
                partial.parent.mkdir(parents=True, exist_ok=True)
                partial.write_bytes(b"segment")
                final.parent.mkdir(parents=True, exist_ok=True)
                final.write_bytes(b"final")
                unrelated.write_bytes(b"other")
                return CommandResult(list(command), 0, "rendered", 0.1)

            full_probe = {
                "path": str(final), "available": True, "bytes": 5,
                "format": "mov,mp4,m4a,3gp,3g2,mj2", "duration_seconds": 1.23456,
                "video": {"codec": "h264", "width": 854, "height": 480, "pixel_format": "yuv420p", "frame_rate": "15/1", "frames": "19"},
                "audio": {"codec": "aac", "sample_rate": "48000", "channels": 2, "layout": "stereo"},
                "raw_ffprobe_payload": "x" * 100_000,
            }
            with patch("manim_director_runtime.rendering.run_command", side_effect=fake_run), patch(
                "manim_director_runtime.media.probe_media", return_value=full_probe
            ):
                result = render_scene({
                    "project_root": raw, "scene": "Demo", "profile": "draft",
                    "manim_executable": "/bin/echo", "media_dir": ".manim-director/media",
                })
            self.assertEqual(result["artifacts"], [str(final)])
            self.assertNotIn("partial_movie_files", json.dumps(result))
            self.assertEqual(len(result["media"]), 1)
            self.assertEqual(result["media"][0]["video"], {"codec": "h264", "width": 854, "height": 480, "fps": 15.0, "pixel_format": "yuv420p", "pix_fmt": "yuv420p", "has_alpha": False})
            self.assertLess(len(json.dumps(result["media"])), 600)
            alpha = compact_media_summary({
                "path": str(final), "bytes": 5, "format": "matroska,webm", "duration_seconds": 1,
                "video": {"codec": "vp9", "width": 854, "height": 480, "frame_rate": "30/1", "pixel_format": "yuv420p", "alpha_mode": "1"},
            })
            self.assertEqual(alpha["format_name"], "matroska,webm")
            self.assertEqual(alpha["video"]["pixel_format"], "yuv420p")
            self.assertEqual(alpha["video"]["pix_fmt"], "yuv420p")
            self.assertTrue(alpha["video"]["has_alpha"])

    def test_diagnostics_classify_common_failures(self) -> None:
        report = diagnose_text('File "/p/scene.py", line 9, in construct\nModuleNotFoundError: No module named \'networkx\'')
        self.assertEqual(report["issues"][0]["code"], "python_import")
        self.assertEqual(report["issues"][0]["module"], "networkx")
        self.assertEqual(report["issues"][0]["location"]["line"], 9)

    def test_math_validation_symbolic_or_numeric(self) -> None:
        result = math_validate({"left": "(x+1)^2", "right": "x^2+2*x+1", "samples": 40})
        self.assertTrue(result["equivalent"])
        false = math_validate({"left": "x+1", "right": "x+2", "samples": 10})
        self.assertFalse(false["equivalent"])
        self.assertAlmostEqual(safe_evaluate("sin(pi/2)+sqrt(9)", {}), 4.0)
        with self.assertRaises(DirectorError):
            math_validate({"left": "__import__('os').system('id')", "right": "0"})

    def test_captions_round_trip_validate_and_reconcile(self) -> None:
        cues = [Cue(0, 1.2, "A small beginning"), Cue(1.3, 2.8, "Then the recurrence grows")]
        encoded = format_captions(cues, "vtt")
        parsed = parse_captions(encoded)
        self.assertEqual([cue.text for cue in parsed], [cue.text for cue in cues])
        self.assertTrue(validate_cues(parsed)["valid"])
        overlap = validate_cues([Cue(0, 2, "one"), Cue(1, 3, "two")])
        self.assertFalse(overlap["valid"])

    def test_asset_manifest_and_export(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "assets").mkdir()
            (root / "assets/data.json").write_text('{"n": 8}\n', encoding="utf-8")
            (root / "assets/manifest.json").write_text(
                json.dumps({"version": 1, "assets": [{
                    "path": "data.json", "origin": "project-authored", "license": "CC0-1.0",
                }]}) + "\n",
                encoding="utf-8",
            )
            (root / "scenes").mkdir()
            (root / "scenes/main.py").write_text(generalized_fibonacci_source(), encoding="utf-8")
            (root / "director.yaml").write_text("version: 1\n", encoding="utf-8")
            manifest = assets({"project_root": raw, "operation": "manifest"})
            self.assertEqual(manifest["count"], 1)
            refreshed = json.loads((root / "assets/manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(refreshed["assets"][0]["origin"], "project-authored")
            self.assertEqual(refreshed["assets"][0]["license"], "CC0-1.0")
            result = export_bundle({"project_root": raw, "output": "output/project.zip"})
            self.assertGreaterEqual(result["files"], 3)
            with zipfile.ZipFile(result["path"]) as archive:
                self.assertIn("scenes/main.py", archive.namelist())
                self.assertIn("manim-director-export.json", archive.namelist())

    def test_default_bundle_follows_generalized_fibonacci_project_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "data").mkdir()
            (root / "assets").mkdir()
            (root / "expected").mkdir()
            (root / "media/videos/scenes/480p15/partial_movie_files/GeneralizedFibonacci").mkdir(parents=True)
            (root / ".manim-director").mkdir()
            (root / "director.yaml").write_text(
                """version: 1
project:
  name: Generalized Fibonacci
  source_dir: .
  asset_dir: assets
  output_dir: dist
  media_dir: .manim-director/media
engine:
  source: scenes.py
inputs:
  data: [data/sequences.csv]
  assets:
    manifest: assets/manifest.json
scenes:
  - file: scenes.py
    depends_on: [data/sequences.csv]
narration:
  manifest: narration.json
captions:
  source: captions.vtt
themes:
  manifest: themes.json
outputs:
  manifest: expected/outputs.json
""",
                encoding="utf-8",
            )
            files = {
                "scenes.py": "from manim import Scene\n",
                "data/sequences.csv": "n,fib\n0,0\n1,1\n",
                "assets/manifest.json": '{"assets":[{"path":"assets/recurrence-knot.svg"}]}\n',
                "assets/recurrence-knot.svg": "<svg xmlns='http://www.w3.org/2000/svg'/>",
                "narration.json": '{"cues":[]}\n',
                "captions.vtt": "WEBVTT\n",
                "themes.json": '{"default":"midnight"}\n',
                "expected/outputs.json": '{"required_sidecars":["captions.vtt","narration.json","director.yaml"]}\n',
                "manim.cfg": "[CLI]\n",
                "requirements.txt": "manim==0.21.0\n",
                "README.md": "# Generalized Fibonacci\n",
                "media/videos/scenes/480p15/final.mp4": "final",
                "media/videos/scenes/480p15/partial_movie_files/GeneralizedFibonacci/segment.mp4": "partial",
                ".manim-director/state.db": "state",
                ".env": "TOKEN=never-export\n",
                "assets/private.key": "never-export",
            }
            for relative, content in files.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8")
            # The stdlib lexical parser must remain fully functional when PyYAML is absent.
            with patch.dict(sys.modules, {"yaml": None}):
                result = export_bundle({"project_root": raw, "format": "bundle", "output": "output/project.zip"})
            with zipfile.ZipFile(result["path"]) as archive:
                members = set(archive.namelist())
            required = {
                "director.yaml", "scenes.py", "data/sequences.csv", "assets/manifest.json",
                "assets/recurrence-knot.svg", "narration.json", "captions.vtt", "themes.json",
                "expected/outputs.json", "manim.cfg", "requirements.txt", "README.md",
                "media/videos/scenes/480p15/final.mp4",
            }
            self.assertTrue(required <= members, sorted(required - members))
            self.assertFalse(any("partial_movie_files" in member for member in members))
            self.assertNotIn(".manim-director/state.db", members)
            self.assertNotIn(".env", members)
            self.assertNotIn("assets/private.key", members)

    def test_bundle_maps_selected_private_render_media_to_deliverables(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "director.yaml").write_text("version: 1\n", encoding="utf-8")
            final = root / ".manim-director/media/videos/main/480p15/Demo.mp4"
            partial = root / ".manim-director/media/videos/main/480p15/partial_movie_files/Demo/part.mp4"
            state = root / ".manim-director/state.db"
            secret = root / ".env"
            for path, content in ((final, b"final"), (partial, b"partial"), (state, b"sqlite"), (secret, b"TOKEN=no")):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(content)
            result = export_bundle({
                "project_root": raw,
                "format": "bundle",
                "output": "output/project.zip",
                "artifacts": [
                    str(final.relative_to(root)), str(partial.relative_to(root)),
                    str(state.relative_to(root)), str(secret.relative_to(root)),
                ],
            })
            with zipfile.ZipFile(result["path"]) as archive:
                members = set(archive.namelist())
                self.assertEqual(
                    archive.read("deliverables/videos/main/480p15/Demo.mp4"),
                    b"final",
                )
            self.assertFalse(any("partial_movie_files" in member for member in members))
            self.assertFalse(any(member.startswith(".manim-director/") for member in members))
            self.assertNotIn(".env", members)

    def test_export_media_copy_and_caption_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            (root / "output").mkdir()
            rendered = root / "output/rendered.mp4"
            rendered.write_bytes(b"project-local-media")
            media = export_bundle({
                "project_root": raw, "format": "mp4", "source": "output/rendered.mp4",
                "output": "output/release.mp4",
            })
            self.assertFalse(media["transcoded"])
            self.assertEqual(Path(media["path"]).read_bytes(), b"project-local-media")
            captions_path = root / "captions.vtt"
            captions_path.write_text("WEBVTT\n\n00:00:00.000 --> 00:00:01.500\nThe roots control growth.\n", encoding="utf-8")
            captions = export_bundle({
                "project_root": raw, "format": "captions", "include": ["captions.vtt"],
                "output": "output/captions.zip",
            })
            self.assertEqual(captions["format"], "captions")
            with zipfile.ZipFile(captions["path"]) as archive:
                self.assertIn("captions/captions.vtt", archive.namelist())
                self.assertIn("captions/captions.srt", archive.namelist())
            with self.assertRaises(DirectorError):
                export_bundle({"project_root": raw, "format": "avi"})

    def test_builtin_templates_compile(self) -> None:
        from manim_director_runtime.themes import get_theme

        for name, (_, generator) in GENERATORS.items():
            source = generator(get_theme("midnight"))
            compile(source, f"{name}.py", "exec")

    def test_visual_qa_and_contact_sheet_when_pillow_available(self) -> None:
        try:
            from PIL import Image, ImageDraw
        except ImportError:
            self.skipTest("Pillow is optional")
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            blank = root / "blank.png"
            visible = root / "visible.png"
            Image.new("RGB", (320, 180), "#101010").save(blank)
            image = Image.new("RGB", (320, 180), "#101010")
            ImageDraw.Draw(image).rectangle((100, 60, 220, 120), fill="#F0F0F0")
            image.save(visible)
            report = analyze_images([blank, visible], metadata={"objects": [{"name": "label", "kind": "text", "font_px": 12, "bbox": [100, 60, 220, 120]}]})
            self.assertEqual(report["status"], "fail")
            self.assertIn("blank_frame", {issue["code"] for issue in report["issues"]})
            self.assertEqual(sum(issue["code"] == "blank_frame" for issue in report["issues"]), 1)
            self.assertIn("tiny_text", {issue["code"] for issue in report["issues"]})
            sheet = create_contact_sheet([blank, visible], root / "sheet.png", columns=2, thumb_width=160)
            self.assertEqual(sheet["images"], 2)
            self.assertTrue((root / "sheet.png").exists())

    def test_child_process_timeout_is_enforced_without_output(self) -> None:
        with self.assertRaises(DirectorError) as raised:
            run_command([sys.executable, "-c", "import time; time.sleep(2)"], timeout=0.08)
        self.assertEqual(raised.exception.code, "command_timeout")

    @unittest.skipIf(os.name == "nt", "Windows uses CTRL_BREAK/taskkill process-group cleanup")
    def test_terminating_bridge_process_cleans_grandchild_group(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            pid_file = Path(raw) / "grandchild.pid"
            parent_code = (
                "import subprocess,sys,time; "
                "child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(30)']); "
                "open(sys.argv[1],'w').write(str(child.pid)); time.sleep(30)"
            )
            helper_code = (
                "from manim_director_runtime.util import run_command; import sys; "
                "run_command([sys.executable,'-c',sys.argv[2],sys.argv[1]],timeout=60)"
            )
            environment = os.environ.copy()
            source_root = str(Path(__file__).resolve().parents[1] / "src")
            environment["PYTHONPATH"] = source_root + os.pathsep + environment.get("PYTHONPATH", "")
            helper = subprocess.Popen(
                [sys.executable, "-c", helper_code, str(pid_file), parent_code], env=environment,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            deadline = time.monotonic() + 5
            while not pid_file.exists() and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(pid_file.exists(), "controllable grandchild did not start")
            grandchild_pid = int(pid_file.read_text(encoding="utf-8"))
            helper.terminate()
            self.assertEqual(helper.wait(timeout=5), 128 + signal.SIGTERM)
            deadline = time.monotonic() + 3
            alive = True
            while alive and time.monotonic() < deadline:
                try:
                    os.kill(grandchild_pid, 0)
                    stat = Path(f"/proc/{grandchild_pid}/stat")
                    if stat.exists() and stat.read_text(encoding="utf-8").split()[2] == "Z":
                        alive = False
                except ProcessLookupError:
                    alive = False
                if alive:
                    time.sleep(0.02)
            self.assertFalse(alive, "grandchild survived bridge termination")

    def test_ingest_normalizes_structured_sources_without_dumping_them(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw)
            incoming = root / "incoming"
            incoming.mkdir()
            markdown = incoming / "notes.md"
            table = incoming / "values.csv"
            payload = incoming / "config.json"
            scene = incoming / "scene.py"
            notebook = incoming / "analysis.ipynb"
            markdown.write_text("# Recurrences\n\n" + "A bounded explanation. " * 200, encoding="utf-8")
            table.write_text("n,value\n0,1\n1,1\n2,2\n", encoding="utf-8")
            payload.write_text('{"series":[1,1,2,3],"name":"fib"}\n', encoding="utf-8")
            scene.write_text("from manim import Scene\nclass Demo(Scene):\n def construct(self): pass\n", encoding="utf-8")
            notebook.write_text(json.dumps({"cells": [{"cell_type": "markdown", "source": ["# Result"], "outputs": []}, {"cell_type": "code", "source": ["1+1"], "outputs": [{"data": {}}]}], "metadata": {"language_info": {"name": "python"}}}), encoding="utf-8")
            result = ingest({"project_root": raw, "paths": [str(markdown), str(table), str(payload), str(scene), str(notebook)], "summary_chars": 300})
            self.assertEqual(result["count"], 5)
            self.assertTrue((root / "sources/manifest.json").exists())
            by_kind = {item["kind"]: item for item in result["sources"]}
            self.assertEqual(by_kind["csv"]["rows"], 3)
            self.assertEqual(by_kind["json"]["shape"]["type"], "object")
            self.assertEqual(by_kind["python"]["scenes"], ["Demo"])
            self.assertEqual(by_kind["notebook"]["cells"], 2)
            self.assertLessEqual(len(by_kind["markdown"]["summary"]), 300)
            self.assertNotIn("A bounded explanation. " * 20, json.dumps(result))


if __name__ == "__main__":
    unittest.main()
