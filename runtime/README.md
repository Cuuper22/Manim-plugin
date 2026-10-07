# Manim Director runtime

The Python half of Manim Director: the bridge worker the engine runs operations in, and the
authoring API (`DirectedScene`) that scenes import.

## Bridge

```bash
python -P -m manim_director_runtime bridge [--preload]
```

One process serves exactly one request (protocol v2):

1. It writes a `ready` frame (runtime version, Manim version, theme and template catalog).
   With `--preload` it imports Manim first, so a pre-warmed worker renders without import latency.
2. It reads one JSON request line from stdin:
   `{"protocol":2,"request_id":"…","method":"render","project_root":"/abs/project","params":{…}}`.
3. It writes `progress` and `log` frames, then exactly one `result` or `error` frame, and exits.

Frames go to the original stdout; everything else that writes to stdout (Manim's console, `print`
in a scene) is redirected to stderr. Methods: `init discover doctor render still frame
contact_sheet qa diagnose validate_math captions ingest export`. Each method's `params` is a typed
task parsed strictly at the boundary (`tasks.py`); unknown or missing fields fail with
`invalid_params` naming the field. Rendering runs Manim in-process; the engine owns timeouts and
cancels by killing the worker's process group.

## Development

```bash
pip install -e '.[full,test]'
pytest
ruff check src tests
```

Manim-backed tests render tiny scenes with Cairo and skip when Manim, FFmpeg or LaTeX are missing.
