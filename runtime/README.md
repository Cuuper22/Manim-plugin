# Manim Director runtime

The runtime is the deliberately small execution layer behind Manim Director. It
does not keep Manim, Pillow, NumPy, or SymPy resident. Each JSONL request loads
only the module it needs and launches render/media tools as bounded child
processes.

```bash
python -m manim_director_runtime bridge
```

Write one request per line:

```json
{"request_id":"1","method":"doctor","params":{"project_root":"."}}
```

The bridge emits zero or more event lines followed by exactly one result or
error line. Standard output contains JSONL only.
