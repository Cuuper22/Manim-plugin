#!/usr/bin/env python3
"""Bridge v2 stand-in for engine tests.

The engine runs `<python> -P -m <module> bridge [--preload]`; tests point
`<python>` at this file, so the module name arrives as an argument and selects
how the process misbehaves. The request's params select how a method does.
Each start and each request is appended to `.manim-director/stub-spawns.txt`
and `stub-requests.txt` (outside every fingerprint) so tests can tell which
process served a job.
"""

import json
import os
import re
import resource
import subprocess
import sys
import time

PROTO = os.fdopen(os.dup(1), "w", encoding="utf-8")
os.dup2(2, 1)


def send(frame):
    PROTO.write(json.dumps(frame, separators=(",", ":")) + "\n")
    PROTO.flush()


def ready(protocol, preload):
    send({
        "type": "ready", "protocol": protocol, "runtime_version": "2.0.0-stub", "python": "3",
        "manim": None, "preloaded": ["numpy"] if preload else [],
        "preload_failed": [{"module": "moderngl", "message": "No module named 'moderngl'"}]
        if preload else [],
        "preload_ms": 1 if preload else 0,
        "catalog": {"themes": [{"name": "midnight", "tokens": [["background", "#0B1020"]]}],
                    "project_templates": ["explainer"], "scene_templates": []},
    })


def record(name, line):
    if os.path.isdir(".manim-director"):
        with open(os.path.join(".manim-director", name), "a", encoding="utf-8") as handle:
            handle.write(line + "\n")


def relative(path, root):
    return os.path.relpath(path, root).replace(os.sep, "/")


def artifact(kind, path, root, label=None):
    return {"kind": kind, "path": relative(path, root), "label": label}


def ffmpeg(*args):
    subprocess.run(["ffmpeg", "-v", "error", "-y", *args], check=True)


def doctor(task, root, rid):
    return {"ok": True, "runtime": {"version": "2.0.0-stub", "protocol": 2, "python": "3",
                                    "executable": sys.executable, "platform": "stub"},
            "checks": [], "capabilities": {"render": True, "renderers": ["cairo"], "latex": False,
                                           "video_tools": True, "visual_qa": True,
                                           "symbolic_math": False, "pdf_ingest": False},
            "disk": {"free_bytes": 1, "total_bytes": 2}, "findings": [], "artifacts": []}


def diagnose(task, root, rid):
    text = task["text"]
    if text == "crash":
        print("stub crashed on purpose", file=sys.stderr)
        sys.stderr.flush()
        os._exit(3)
    if text == "sleep":
        time.sleep(60)
    if text == "wrong-id":
        send({"type": "log", "request_id": "someone-else", "level": "info", "message": "?"})
    if text == "huge":
        send({"type": "log", "request_id": rid, "level": "info", "message": "x" * (1100 * 1024)})
    if text == "error":
        raise StubError("render_failed", "Scene raised NameError.", {"stage": "construct"})
    if text == "unknown-code":
        raise StubError("exploded", "Something odd.", None)
    if text == "null-id":
        send({"type": "error", "request_id": None,
              "error": {"code": "invalid_request", "message": "Request is not valid JSON.",
                        "data": None}})
        return None
    if text == "rlimit":
        soft = resource.getrlimit(resource.RLIMIT_AS)[0]
        text = "unlimited" if soft == resource.RLIM_INFINITY else str(soft)
    if text == "chatty":
        for index in range(500):
            print(f"chatter {index}", file=sys.stderr)
    print("a print from user code")
    send({"type": "progress", "request_id": rid, "phase": "analyze", "current": 1, "total": 1,
          "scene_seconds": None, "message": None})
    send({"type": "log", "request_id": rid, "level": "warning", "message": "looked at the text"})
    finding = {"code": "unclassified", "severity": "info", "message": text[:200], "hint": None,
               "location": None, "at_seconds": None, "beat": None, "frame": None}
    return {"recognized": False, "findings": [finding], "artifacts": []}


def validate_math(task, root, rid):
    pairs = [{"index": i, "equivalent": True,
              "symbolic": {"available": False, "equivalent": None, "difference": None},
              "numeric": {"samples_valid": task["samples"], "samples_skipped": 0,
                          "max_abs_error": 0.0, "max_rel_error": 0.0, "counterexample": None}}
             for i in range(len(task["steps"]) - 1)]
    return {"valid": True, "variables": sorted(task["ranges"]), "pairs": pairs, "artifacts": []}


def captions(task, root, rid):
    artifacts = []
    if task["output"]:
        os.makedirs(os.path.dirname(task["output"]), exist_ok=True)
        with open(task["path"], encoding="utf-8") as source, \
                open(task["output"], "w", encoding="utf-8") as output:
            output.write(source.read())
        artifacts.append(artifact("captions", task["output"], root))
    return {"cue_count": 1, "duration_seconds": 1.0, "valid": True, "findings": [],
            "artifacts": artifacts}


def render(task, root, rid):
    settings = task["settings"]
    scene = task["scene"] or "Scene"
    if scene == "SlowScene":
        time.sleep(60)
    os.makedirs(task["out_dir"], exist_ok=True)
    video = os.path.join(task["out_dir"], f"{scene}.{settings['format']}")
    ffmpeg("-f", "lavfi", "-i",
           f"color=c=navy:s={settings['width']}x{settings['height']}:r={settings['fps']}:d=1",
           "-pix_fmt", "yuv420p", video)
    timeline = os.path.join(task["out_dir"], f"{scene}.timeline.json")
    with open(timeline, "w", encoding="utf-8") as handle:
        json.dump({"version": 1, "scene": scene, "duration_seconds": 1.0,
                   "beats": [{"id": "hook", "start_seconds": 0.0, "end_seconds": 1.0,
                              "file": relative(task["files"][0], root), "line": 3}]}, handle)
    return {"scene": {"name": scene, "file": relative(task["files"][0], root)},
            "duration_seconds": 1.0, "animations": 1,
            "artifacts": [artifact("video", video, root), artifact("timeline", timeline, root)]}


def frame(task, root, rid):
    image = os.path.join(task["out_dir"], "frame.png")
    ffmpeg("-ss", str(task["at_seconds"]), "-i", task["video"], "-frames:v", "1", image)
    return {"at_seconds": task["at_seconds"], "artifacts": [artifact("image", image, root)]}


def discover(task, root, rid):
    with open(os.path.join(root, "discover-calls.txt"), "a", encoding="utf-8") as calls:
        calls.write("call\n")
    scenes = []
    for path in task["files"]:
        with open(path, encoding="utf-8") as handle:
            lines = handle.readlines()
        for number, line in enumerate(lines, 1):
            match = re.match(r"class (\w+)\(", line)
            if match:
                scenes.append({"name": match.group(1), "file": relative(path, root),
                               "line": number, "end_line": number, "construct_line": None,
                               "bases": ["Scene"], "doc": None, "theme": None,
                               "sections": [], "beats": []})
        if "# edited during the scan\n" in lines:
            with open(path, "a", encoding="utf-8") as handle:
                handle.write("class Late(Scene):\n    pass\n")
    return {"truncated": False, "scenes": scenes, "findings": [], "artifacts": []}


def init(task, root, rid):
    os.makedirs(os.path.join(root, "scenes"), exist_ok=True)
    name = task["name"] or "Project"
    files = {"director.yaml": f"version: 1\nproject:\n  name: {name}\n",
             "scenes/main.py": "class MainScene(Scene):\n    pass\n"}
    for path, content in files.items():
        with open(os.path.join(root, path), "w", encoding="utf-8") as handle:
            handle.write(content)
    return {"mode": task["mode"], "name": name, "slug": name.lower(), "seed": 1,
            "template": task["template"], "scene_template": None, "theme": "midnight",
            "scene": {"name": "MainScene", "file": "scenes/main.py"},
            "artifacts": [artifact("file", os.path.join(root, path), root) for path in files]}


class StubError(Exception):
    def __init__(self, code, message, data):
        super().__init__(message)
        self.body = {"code": code, "message": message, "data": data}


METHODS = {"doctor": doctor, "diagnose": diagnose, "validate_math": validate_math,
           "captions": captions, "render": render, "frame": frame, "discover": discover,
           "init": init}


def main():
    module = sys.argv[sys.argv.index("-m") + 1]
    preload = "--preload" in sys.argv
    record("stub-spawns.txt", f"{os.getpid()} {'preload' if preload else 'plain'}")
    if module == "stub_without_ready":
        print("usage: unknown option --preload", file=sys.stderr)
        return 2
    if module == "stub_dies_idle":
        # Nobody reads stdin any more, so a request write fails at once.
        os.close(0)
        ready(2, preload)
        time.sleep(0.1)
        return 0
    ready(1 if module == "stub_protocol_1" else 2, preload)
    line = sys.stdin.readline()
    if not line.endswith("\n"):
        return 0
    request = json.loads(line)
    rid = request["request_id"]
    record("stub-requests.txt", f"{os.getpid()} {rid}")
    method = request["method"]
    if method not in METHODS:
        send({"type": "error", "request_id": rid,
              "error": {"code": "unknown_method", "message": method, "data": {"method": method}}})
        return 0
    try:
        result = METHODS[method](request["params"], request["project_root"], rid)
    except StubError as error:
        send({"type": "error", "request_id": rid, "error": error.body})
        return 0
    if result is not None:
        send({"type": "result", "request_id": rid, "result": result})
    return 0


if __name__ == "__main__":
    sys.stdout.flush()
    code = main()
    PROTO.flush()
    os._exit(code)
