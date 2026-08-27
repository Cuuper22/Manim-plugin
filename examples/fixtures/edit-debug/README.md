# Edit/debug fixture

`editable_scene.py` is a valid, deterministic scene whose visible choices live in `fixture.json`. It lets an edit workflow prove that a natural-language change touched only the requested keys.

```bash
manim -pql editable_scene.py EditablePulse
```

`broken_scene.py` intentionally requests one nonexistent SVG. A debugger should name `missing-badge.svg`, scope the failure to `MissingAssetScene.construct`, and produce the asset-free result in `expected/repaired_scene.py` without rewriting the healthy fixture.

```bash
manim -pql broken_scene.py MissingAssetScene
manim -pql expected/repaired_scene.py MissingAssetScene
```
