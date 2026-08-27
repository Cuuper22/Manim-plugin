# Project and animation specification

Read this reference when creating a project, changing output profiles, or reconciling project metadata. `director.yaml` is the durable creative and execution contract; Manim Python remains the executable source of truth for scene behavior.

## Canonical layout

```text
project/
├── director.yaml
├── manim.cfg
├── requirements.txt
├── scenes/
├── assets/
│   └── manifest.json
├── sources/
│   └── manifest.json
├── narration/
├── captions/
├── output/
└── .manim-director/
    ├── media/
    ├── tmp/
    └── state.db
```

Keep source assets separate from generated output and engine state. Cache/media below `.manim-director/` is reproducible and may be cleaned; `state.db` contains the job index and cursor events, not source-of-truth creative content.

## Rich version-1 schema

Only `version`, `project`, `render`, `theme`, `safe_area`, and `captions` are needed for a small project. Add the creative sections that materially help the work.

```yaml
version: 1
schema: manim-director/v1

project:
  name: Generalized Fibonacci
  id: generalized-fibonacci
  title: Generalized Fibonacci Sequences
  description: A visual tour of second-order linear recurrences.
  language: en-US
  seed: 1729
  source_dir: scenes
  asset_dir: assets
  output_dir: output
  media_dir: .manim-director/media

brief:
  objective: Show how coefficients and initial values change the recurrence.
  audience: Curious adults comfortable with algebra
  rigor: intuition-then-derivation
  duration_seconds: 150
  assumptions: []
  required_claims: [dominant-root]
  forbidden_elements: []

engine:
  backend: manim-ce       # manim-ce | manimgl
  source: scenes/main.py
  main_scene: RecurrenceScene
  compatible: ">=0.19,<1"
  fallback: manim -pql scenes/main.py RecurrenceScene

render:                    # active/default render target
  renderer: cairo
  profile: preview
  format: mp4
  transparent: false
  width: 1920
  height: 1080
  fps: 60

theme:
  preset: midnight
  background: "#0B1020"
  foreground: "#F7F8FC"
  primary: "#78DCE8"
  secondary: "#FFD866"
  accent: "#FF6188"
  muted: "#72798C"
  success: "#A9DC76"
  font: DejaVu Sans
  math_font_size: 48
  text_font_size: 36
  stroke_width: 4

safe_area:
  top: 0.05
  right: 0.05
  bottom: 0.08
  left: 0.05

captions:
  format: vtt
  burn_in: false
  source: captions/captions.vtt
  safe_area_percent: 8

inputs:
  data: [sources/sequences.csv]
  assets:
    manifest: assets/manifest.json
  sources:
    - id: measurements
      path: sources/sequences.csv
      kind: dataset
  claims:
    - id: dominant-root
      text: The dominant characteristic root controls asymptotic growth.
      status: exact
      assumptions: [distinct-roots, nonzero-leading-coefficient]
      evidence: tests/test_recurrence.py::test_dominant_root

storyboard:
  - id: hook
    objective: Establish Fibonacci as one member of a family.
    visual: A concrete sequence opens into a parameterized recurrence.
    duration: 18
    narration_cue: cue-01

scenes:
  - id: recurrence
    class: RecurrenceScene
    file: scenes/main.py
    purpose: Build the recurrence from concrete terms.
    duration_seconds: 38
    sections: [hook, construction, generalization]
    depends_on: []

narration:
  manifest: narration/manifest.json
  timing: cue
  mode: recorded          # none | recorded | tts
  source: narration/voice.wav

profiles:
  preview:
    resolution: [854, 480]
    fps: 24
    renderer: cairo
    format: mp4
    quality: low
  production:
    resolution: [1920, 1080]
    fps: 60
    renderer: cairo
    format: mp4
    quality: high
  vertical:
    resolution: [1080, 1920]
    fps: 60
    renderer: cairo
    format: mp4
    quality: high
    layout: responsive
  transparent:
    resolution: [1920, 1080]
    fps: 30
    renderer: cairo
    format: mov
    alpha: true
  orbit-3d:
    resolution: [1280, 720]
    fps: 30
    renderer: opengl
    format: mp4
    scenes: [StateOrbit3D]

validation:
  math:
    tolerance: 1.0e-9
  visual:
    minimum_text_px: 28
    minimum_contrast: 4.5
    safe_area_percent: 8
  assertions:
    - fibonacci[7] == 13

outputs:
  manifest: output/manifest.json

budgets:
  render_seconds: 1800
  memory_mb: 8192
  output_mb: 2048

extensions: {}
```

The parser preserves unknown keys at the top level and inside typed records. Put namespaced third-party data under `extensions` unless it is intentionally part of a standard section.

## Meaning of the major records

- `project` supplies identity, deterministic seed, and project-relative roots.
- `brief` records the creative objective and what kind of claim the animation promises.
- `engine` names the Manim flavor, entry file, primary scene, and compatible version range.
- `render` is the active default used when a named profile does not override it.
- `profiles` holds reusable variants. A different aspect ratio is a layout variant, not a crop.
- `storyboard` contains viewer-facing beats. `scenes` inventories executable scene classes; several beats may live in one scene.
- `inputs` records source-backed data/assets and claims. It points to source files instead of embedding large contents.
- `validation` describes checks whose results belong in QA reports.
- `budgets` bounds expensive jobs without weakening the requested output profile.

## Field invariants

- `version` is `1`; `schema: manim-director/v1` is an optional descriptive compatibility label.
- `project.name` is required. For older rich specs, the loader can derive it from `project.title` or `project.id`, but new specs write it explicitly.
- `project.source_dir`, `asset_dir`, `output_dir`, and `media_dir` are relative directories without parent traversal. `engine.source` and other file references are project-relative.
- Scene IDs, scene classes, beat IDs, section names, source IDs, and claim IDs remain stable after cues/references are attached.
- Output dimensions and FPS are positive. The format supports every requested stream and alpha requirement.
- Durations include transitions and intentional holds. Beat totals remain within five percent of `brief.duration_seconds` unless runtime is explicitly flexible.
- Safe-area values are fractions from `0` through `1`; opposing margins leave a positive content region.
- A seed is required whenever randomness affects visible output.
- Secrets and provider tokens never appear in the spec.

## Claims and sources

A claim may be a short string for compatibility or a structured record. Prefer structured claims when correctness matters. Allowed status values are `exact`, `approximation`, `intuition`, and `conjecture`. Evidence is a narrow assertion, source reference, or derivation identifier—not a pasted log.

Source hashes are optional cache/provenance metadata. Use them when content identity matters; do not surface them as ceremonial proof that an animation is correct.

If sources disagree, record the conflict and stop before asserting either value as fact. If validation cannot establish a claim, downgrade its status or state the missing assumption.

## Editing rules

For an existing project, merge only changed fields. Preserve unknown keys, extensions, scene ordering, output names, manual theme overrides, and brand data. Changing a stable ID invalidates attached cues/cache references and should be explicit.

For aspect-ratio or localization variants, share calculations and semantic beats while allowing profile-specific composition, wrapping, caption placement, and camera framing. Formulas and registered data values remain consistent across variants.
