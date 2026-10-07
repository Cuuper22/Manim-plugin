# director.yaml

Read this when creating a project's spec or changing its theme, storyboard, profiles or layout
margins. Only `project.name` is required; unknown keys are ignored. An invalid file fails renders
with `invalid_spec` and the YAML line, so validate edits by calling `inspect` afterwards.

```yaml
version: 1
project:
  name: Completing the square
  seed: 7                          # seeds anything random in the scenes
  source_dir: scenes               # where scenes live; "." for the root
  asset_dir: assets
  output_dir: output               # exports
engine:
  source: scenes/main.py
  main_scene: QuadraticFormula     # the default `scene`
theme: paper                       # midnight | paper | chalkboard | contrast
direction:
  symbols:                         # TeX token -> theme token or #RRGGBB, for every scene
    a: primary
    b: secondary
    c: accent
safe_area: {top: 0.05, right: 0.05, bottom: 0.08, left: 0.05}
render:
  profile: preview                 # default profile
profiles:
  loop-gif: {resolution: [640, 360], fps: 15, format: gif}
  transparent: {quality: high, format: mov, alpha: true}
storyboard:
  - id: claim                      # equals the beat id in the scene
    intent: introduce              # introduce | explain | compare | reveal | prove | recap
    audience_question: Which x solve ax^2 + bx + c = 0?
    takeaway: Any quadratic with a nonzero leading coefficient.
  - id: complete
    intent: prove
    transition: continue           # continue | contrast | reveal | chapter
    audience_question: How can the left side become a perfect square?
    takeaway: Adding (b/2a)^2 to both sides completes the square.
```

- **Storyboard.** One entry per beat, same ids and order as the `with self.beat("…")` calls. Also
  accepted: `focus`, `visual_metaphor`, `duration`. It is the plan readers and the workbench see;
  the scene code is what renders.
- **Scenes.** Every `*.py` under `source_dir` (plus `engine.source`) is scanned for scene classes.
  `scenes: [{id, class, file, purpose, duration_seconds}]` gives scenes stable ids that `scene`
  accepts.
- **Profiles.** Built-ins: `draft` 854×480@15, `preview` 1280×720@30, `production` 1920×1080@60,
  `ultra` 3840×2160@60, `custom` (from `render.width/height/fps`). An entry may override a built-in
  or add a new name with `quality` (Manim's: `low` 480p15, `medium` 720p30, `high` 1080p60,
  `production` 1440p60, `fourk` 2160p60; not the profiles of the same name), `resolution`
  `[w, h]` (even, 16–8192), `fps` (1–240), `renderer` (`cairo`, `opengl`), `format` (`mp4`, `mov`,
  `webm`, `gif`) and `alpha` (needs `mov` or `webm`).
- **Safe area.** Fractions of the frame kept clear on each side (0–0.45). Regions, `place()` and
  `qa`'s `safe_area` check all use it. Vertical video for phones wants more at the bottom and right
  (see the `vertical_short` template).
- **Theme and symbols.** A scene's `theme`/`symbols` class attributes override these per scene.
- **Inputs.** `inputs.data` and `inputs.sources` list data files the scenes read, `captions.source`
  and `narration.manifest` the caption and narration files; zip exports include them.
- **Budgets.** `budgets.render_seconds` (job timeout, default 1800), `budgets.output_mb` (artifact
  size, default 2048), `budgets.memory_mb` (Unix address-space limit, off by default).

Portrait output also needs `manim.cfg` with `pixel_width`, `pixel_height` and `frame_rate` in the
same shape, or `DirectedScene` refuses to render squashed.
