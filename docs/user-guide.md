# User guide

Manim Director gives one Manim project three interfaces:

- `manim-director` for terminal workflows;
- the local React workbench for visual navigation and revision-safe source edits; and
- the `$manim-director` Codex skill plus MCP tools for natural-language creation, repair, QA, and delivery.

The source remains ordinary, editable Manim Python. Director adds a project spec, bounded job runner, caching, inspection, media utilities, and reproducible export; it does not lock the project into a proprietary scene format.

## Install

Installation requires Python 3.11 or newer. The default path downloads the platform release binary with the workbench embedded; a local source build additionally requires Rust 1.82 or newer and Node.js/npm. Manim rendering also needs the native dependencies required by the chosen Manim renderer and output, normally FFmpeg plus Cairo/Pango; LaTeX, Typst, fontconfig, and OpenGL are optional capabilities discovered by `doctor`.

```bash
git clone https://github.com/Cuuper22/Manim-plugin.git
cd Manim-plugin
python3 scripts/install.py --with-manim
```

The script downloads the matching release, verifies its exact SHA-256 against the release `SHA256SUMS` asset before archive extraction, creates an isolated runtime environment under the same prefix, installs the constrained visual/math/Manim dependency set there, and writes the binary under `~/.local/bin` by default. Linux downloads use a static musl build, and each archive carries `LICENSE` plus `THIRD_PARTY_NOTICES.md`. The binary discovers that prefix-local interpreter automatically, avoiding user-site and active-virtualenv ambiguity; `MANIM_DIRECTOR_PYTHON` remains an explicit override. On a supported checkout with Cargo and npm available, a failed release download falls back to a local build. If needed:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Build the current checkout instead of downloading a release, or choose another prefix:

```bash
python3 scripts/install.py --with-manim --from-source
python3 scripts/install.py --with-manim --prefix /opt/manim-director
```

Add the Codex plugin after the host binary is on `PATH`:

```bash
codex plugin marketplace add Cuuper22/Manim-plugin
codex plugin add manim-plugin@manim-director
```

Open a fresh Codex thread after installation so the skill and MCP declaration load together.

For source development, use:

```bash
make check
make dev
```

`make dev` runs the Rust API on port 4177 and Vite on port 4173 with API proxying.

## First project

```bash
mkdir recurrence-film
cd recurrence-film
manim-director init --name "Recurrence Film"
manim-director doctor
manim-director preview --scene MainScene
manim-director render --scene MainScene --profile production
```

`init` creates `director.yaml`, a runnable `scenes/main.py`, project directories, generated-state directories, and `.gitignore`. It refuses any destination containing user project files unless `--force` is explicit; Git and empty Director metadata are preserved automatically.

You can target a project from anywhere:

```bash
manim-director --project /work/recurrence-film inspect
```

The `--project` option may name the root or any path below it; Director walks upward to `director.yaml`.

## Use it from Codex

The skill activates for Manim work, or can be invoked explicitly as `$manim-director`. Good prompts describe the human outcome and constraints, not low-level mobjects:

```text
$manim-director Create a 75-second 16:9 explanation of why the
characteristic roots control a generalized Fibonacci sequence. Build intuition
before derivation, include the repeated-root edge case, preview it, inspect the
opening/main transition/final frame, then export editable source and MP4.
```

```text
$manim-director In this existing project, make only the roots scene vertical,
keep my custom updater code, move captions above the platform safe area, and
rerender just that scene.
```

```text
$manim-director The third scene crashes. Diagnose it, make the smallest fix,
preview the repaired section, and show me the output and remaining QA findings.
```

The agent should persist decisions in `director.yaml` and project files, request compact job state, inspect actual frames through an image viewer, and return artifact paths. It should not pull videos, full source trees, or unbounded logs into text context.

## Project specification

The scaffold writes a compact valid spec. More developed projects can add a brief, input provenance, storyboard, scene catalog, narration, named profiles, validation rules, accessibility variants, and deliverable manifests. Unknown extension data is preserved.

```yaml
version: 1
schema: manim-director/v1
project:
  name: Recurrence Film
  source_dir: scenes
  asset_dir: assets
  output_dir: output
  media_dir: .manim-director/media
  language: en-US
  seed: 73
brief:
  objective: Explain second-order linear recurrences visually.
  audience: Algebra-comfortable adults
  rigor: intuition-then-derivation
  duration_seconds: 75
engine:
  backend: manim-ce
  source: scenes/main.py
  main_scene: MainScene
  compatible: ">=0.19,<1"
render:
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
  foreground: "#F5F7FF"
  accent: "#69D2FF"
  font: Inter
safe_area: {top: 0.05, right: 0.05, bottom: 0.08, left: 0.05}
captions:
  format: vtt
  burn_in: false
profiles:
  vertical:
    resolution: [1080, 1920]
    fps: 30
    renderer: cairo
    format: mp4
    layout: responsive
budgets:
  render_seconds: 1800
  memory_mb: 8192
  output_mb: 2048
```

The four project paths must stay relative and may not contain `..`. `.manim-director/` holds generated media, temporary files, undo snapshots, and SQLite state. Do not hand-edit `state.db`.

## Inspect and explain

A fast inventory does not launch Python:

```bash
manim-director inspect
manim-director --json inspect
```

Deep inspection also asks the Python runtime to parse scene files and discover runtime capabilities:

```bash
manim-director inspect --deep
```

Use fast inspect to list source/assets/outputs or understand the project shape. Use deep inspect when scene discovery, syntax, Manim flavor, or Python-side details matter.

To ask Codex for an explanation without changing anything:

```text
$manim-director Explain the object lifecycle and math in CharacteristicRoots.
Do not render unless the animation behavior cannot be determined from source.
```

## Environment doctor

```bash
manim-director doctor
manim-director --json doctor
```

Doctor reports the selected Python, platform, installed package versions, and discovered executables/capabilities including Manim, FFmpeg/ffprobe, LaTeX, Typst, and font tooling. A missing optional tool is not fatal unless the requested project feature requires it.

Override runtime selection when multiple Python environments exist:

```bash
MANIM_DIRECTOR_PYTHON=/work/venv/bin/python manim-director doctor
```

The bridge module can be overridden for runtime development:

```bash
MANIM_DIRECTOR_RUNTIME_MODULE=manim_director_runtime manim-director doctor
```

## Preview and render

### Profiles

The Python runtime has four built-in profiles:

| Profile | Resolution | FPS | Manim quality |
|---|---:|---:|---|
| `draft` | 854×480 | 15 | low |
| `preview` | 1280×720 | 30 | medium |
| `production` | 1920×1080 | 60 | high |
| `ultra` | 3840×2160 | 60 | 4K |

Named profiles in `director.yaml` can supply project-specific resolution, renderer, format, alpha, scene selection, and responsive layout metadata.

### Common commands

```bash
# Fast timing/motion check
manim-director preview --scene MainScene

# Preview one declared section and request representative frames
manim-director preview --scene MainScene --section roots --contact-sheet

# Production render using Cairo
manim-director render --scene MainScene --profile production --renderer cairo

# OpenGL scene
manim-director render --scene StateOrbit3D --profile preview --renderer opengl

# Transparent MOV (MP4 and GIF cannot preserve full alpha)
manim-director render --scene Overlay --transparent \
  --set format='"mov"' --set output='"output/overlay.mov"'
```

When a file contains multiple scenes, specify `--scene`, or pass `--set all_scenes=true`. Select a non-default file with:

```bash
manim-director render --scene ProofScene \
  --set scene_file='"scenes/proof.py"'
```

`--set KEY=JSON` is the escape hatch shared by commands. Valid JSON is decoded; other text becomes a string. Quoting JSON strings explicitly avoids shell ambiguity.

Custom resolution and FPS:

```bash
manim-director render --scene MainScene --profile custom \
  --set width=1080 --set height=1920 --set fps=30
```

Custom values are bounded to 16–16384 pixels and 1–240 FPS.

Director refuses to claim success when Manim exits without producing the requested media. A successful result lists each artifact and, when ffprobe is available, its media metadata.

## Create and edit

There are three practical editing surfaces:

1. Ask the Codex skill to change a named scene/object/beat in natural language.
2. Use the workbench source editor while previewing the project.
3. Use the source API/CLI for revision-checked automation.

Scope an agent edit tightly:

```text
$manim-director In scenes/roots.py, hold only the final closed form two seconds
longer. Preserve every other scene and reconcile its caption cue. Preview the
roots section after the edit.
```

Director's source mutation layer supports full replacement, a 1-based inclusive line replacement, and JSON Merge Patch for `director.yaml`. Every mutation:

- is limited to a 2 MiB source file;
- is confined to approved text/source extensions under the project;
- can require the revision returned by the read;
- validates Python syntax, project YAML, or JSON before committing;
- writes atomically; and
- snapshots the previous file beneath `.manim-director/undo/`.

This is optimistic concurrency: if the file changed after it was read, the write fails instead of overwriting the newer version.

CLI examples:

```bash
# Replace a complete file from a prepared source file
manim-director edit scenes/main.py --content-file /tmp/new-main.py \
  --expected-revision 763a5f...

# Replace lines 40 through 43, inclusive
manim-director edit scenes/main.py --line 40:43 \
  --replacement-file /tmp/replacement.py \
  --expected-revision 763a5f...

# Patch only one project-spec field
manim-director edit director.yaml \
  --merge-patch '{"render":{"profile":"production"}}' \
  --expected-revision c82417...
```

For short changes, `--content` and `--replacement` accept inline text. For larger or multiline changes, the file variants avoid shell quoting problems. A successful response includes the new revision, undo snapshot path, and scene IDs whose declared source file changed.

The workbench reads a revision and bounded source span through `GET /api/state/source`, then writes through `PUT /api/state/source`. MCP exposes the same mutation contract as `project_apply`.

## Visual QA

QA accepts a rendered video or still. It extracts representative video frames when needed and reports timestamped blank/near-blank frames, low contrast, safe-area intrusion, and—in projects that supply object metadata—tiny text, clipping, and likely object overlap.

```bash
manim-director qa --artifact output/final.mp4

manim-director qa --artifact output/final.mp4 \
  --set frame_count=12 --set minimum_text_px=24
```

A `pass` means the implemented checks found no issue. `warn` or `fail` is evidence for visual review, not a command to blindly alter the scene. Open the relevant frame and make a scoped judgment before repair.

The normal production loop is:

```text
edit → targeted preview/still → representative-frame QA → scoped repair → final render
```

## Debug

Diagnose a scene:

```bash
manim-director debug --scene MainScene
```

Diagnose an existing failed job:

```bash
manim-director debug --job-id d5f8a292-cb9b-495b-a851-4cb2c9def9f3
```

Diagnostics normalize common Python tracebacks, missing executables/assets/fonts, LaTeX/Typst failures, renderer problems, and media errors into a stable code plus evidence. The debug command diagnoses; an agent or user applies the source fix, then previews only the affected scope.

Useful failures:

| Code/message | Action |
|---|---|
| `project_not_found` / no `director.yaml` | Run from the project or pass `--project`; initialize a new root if intended. |
| `scene_not_found` | Check `inspect --deep`; pass the exact class and correct `scene_file`. |
| `scene_required` | Supply `--scene` or `--set all_scenes=true`. |
| `executable_not_found` | Run `doctor`; select the correct Python or install the missing native capability. |
| `render_failed` | Read structured diagnostics and the bounded tail, then debug the named stage. |
| `render_output_missing` | Check format/media path and Manim compatibility; process exit alone is not success. |
| `path_outside_project` | Choose a project-relative input/output path. |
| `revision conflict` | Read the new revision and reapply the intended change. |
| `queue_full` | Wait for an active job; do not blindly increase worker count. |
| `job_timeout` | Narrow the render or deliberately increase the timeout budget. |

Set Rust logging without contaminating machine JSON on stdout:

```bash
RUST_LOG=manim_director=info manim-director render --scene MainScene
```

## Assets

Assets can be inventoried, copied into the project, or normalized. Normalization can sanitize SVG, resize/orient raster images, or normalize audio through FFmpeg. Metadata can retain source, license, attribution, and notes in `assets/manifest.json`.

Advanced operations are available through an explicit intent while the server runs:

```bash
curl -sS http://127.0.0.1:4177/api/intents \
  -H 'content-type: application/json' \
  -d '{
    "intent":"normalize the selected SVG",
    "operation":"assets",
    "params":{
      "operation":"normalize",
      "source":"/tmp/diagram.svg",
      "destination":"assets/diagram.svg",
      "license":"CC-BY-4.0"
    }
  }'
```

The source is allowed outside the project for deliberate import; the destination is not.

## Source ingestion

Ingest notes, papers, datasets, code, notebooks, vector/raster references, audio, or video before storyboarding:

```bash
# CLI
manim-director ingest /incoming/brief.md /incoming/sequences.csv \
  --set destination_dir='"sources"' --set summary_chars=4000

# Equivalent async REST submission
curl -sS http://127.0.0.1:4177/api/ingest \
  -H 'content-type: application/json' \
  -d '{
    "paths":["/incoming/brief.md","/incoming/sequences.csv"],
    "params":{"destination_dir":"sources","summary_chars":4000}
  }'
```

The operation copies material into the project, creates bounded summaries instead of dumping full documents into agent context, and records descriptors in `sources/manifest.json`. A descriptor can retain an ID, kind, provenance, license, attribution, and notes. PDF text extraction requires the Python runtime's `ingest` extra and is included by `--with-manim`/`full`.

From Codex/MCP, `project_apply` accepts `{"ingest":["/incoming/brief.md", ...]}` and returns the ingest job resource.

## Math validation, captions, and media helpers

The same intent endpoint exposes non-CLI operations without adding dozens of commands:

```bash
# Symbolic/numeric math validation
curl -sS http://127.0.0.1:4177/api/intents \
  -H 'content-type: application/json' \
  -d '{"intent":"validate recurrence","operation":"validate_math","params":{"kind":"equivalence","left":"(x+1)^2","right":"x^2+2*x+1"}}'

# Caption validation/export
curl -sS http://127.0.0.1:4177/api/intents \
  -H 'content-type: application/json' \
  -d '{"intent":"validate captions","operation":"captions","params":{"operation":"validate","source":"captions.vtt"}}'
```

These return a job. Follow it through `/api/events`, `GET /api/renders/{id}`, the workbench queue, or MCP `job_status`.

## Workbench

Open the production workbench and browser:

```bash
manim-director open
```

Keep the browser closed or choose a fixed address:

```bash
manim-director open --no-browser
manim-director serve --host 127.0.0.1 --port 4177
```

Use the workbench to navigate source and assets, play previews, inspect scenes/beats/objects, route render/preview/QA/export/inspect intents, edit revision-checked source, monitor render progress, cancel jobs, read bounded logs, and download export artifacts. Free-form semantic source edits are performed by the Codex skill through `project_apply`; the local workbench does not pretend to contain a language model.

The server is intentionally project-scoped. Starting it for a different project requires another process/port. Do not bind `--host 0.0.0.0` on an untrusted network; there is no built-in login.

If the installed UI assets are in a nonstandard location:

```bash
manim-director serve --workbench-dir /opt/manim-director/share/manim-director/workbench

# Equivalent persistent selection
MANIM_DIRECTOR_WORKBENCH=/that/path manim-director open
```

## Export

Create a reproducible source bundle:

```bash
manim-director export --format zip \
  --output output/recurrence-film-source.zip
```

The default bundle includes `director.yaml`, Manim config, requirements, README, scenes, assets, and output. It excludes Git state, Director job/cache state, Python caches, symlinks, and platform junk. The ZIP contains `manim-director-export.json` with the file list and uncompressed sizes.

The default uncompressed input budget is 2 GiB and can be lowered per job:

```bash
manim-director export --output output/source.zip --set max_bytes=536870912
```

The result contains the final archive path, compressed/uncompressed bytes, and file count. Review licenses, attribution, confidential source, narration, and final media before distributing it.

Export a successful render by job ID as MP4, WebM, GIF, or a caption sidecar package:

```bash
manim-director export --format webm --job-id <render-job-uuid> --output output/final.webm
manim-director export --format gif --job-id <render-job-uuid> --output output/loop.gif
manim-director export --format captions --job-id <caption-job-uuid> --output output/captions.zip
```

GIF frame delays are integer centiseconds. Director selects and reports the nearest representable cadence (for example, requested 15 fps becomes `100/7`, about 14.286 fps) and validates the file against that declared effective rate.

## Machine use and local API

Add `--json` for one-line machine-readable CLI output:

```bash
manim-director --json inspect
manim-director --json render --scene MainScene --profile preview
```

Run the local service for asynchronous integrations:

```bash
manim-director serve --port 4177
```

Core endpoints:

```text
GET  /api/health
GET  /api/state
GET  /api/state/source
PUT  /api/state/source
GET  /api/files?path=...
POST /api/intents
POST /api/ingest
POST /api/renders
GET  /api/renders/{id}
POST /api/renders/{id}/cancel
POST /api/exports
GET  /api/logs
GET  /api/events              (SSE)
```

See [protocol.md](protocol.md) for exact bodies, job shapes, events, cursors, and MCP resources.

## Runtime tuning

```bash
MANIM_DIRECTOR_WORKERS=2 \
MANIM_DIRECTOR_QUEUE=128 \
MANIM_DIRECTOR_TIMEOUT_SECONDS=3600 \
manim-director serve
```

Defaults are already conservative. Set workers to one on a memory-limited machine. More workers help independent small jobs; they often hurt when several Manim/FFmpeg renders already saturate CPU or RAM.

Cached operations use relevant project inputs and operation parameters. If an undeclared external tool/input changed, bypass both Director and Manim caches for that render with `--set disable_caching=true`, or deliberately clear them with `--set flush_cache=true`.

## Platform notes

### Linux

Cairo is the most predictable renderer for desktop and headless use. OpenGL needs a working graphics/display or EGL setup. Ensure `~/.local/bin` is on `PATH` after the default install.

### macOS

Use a native Rust toolchain and Python environment. Ensure FFmpeg and any TeX/Typst executables are visible to GUI-launched processes as well as the shell. `open` uses the system `open` command.

### Windows

Use a Rust MSVC toolchain and an installed Python reachable as `python`/the configured runtime. The binary is `manim-director.exe`; browser launch uses `cmd /C start`. PowerShell quoting differs, so use a JSON file/API client for elaborate `--set` values. Keep project paths reasonably short for older TeX/FFmpeg components.

### Containers and CI

Build the workbench once, run the release binary, mount exactly one project root writable, and preinstall the native render stack. Keep the API on loopback or protect it at the ingress. For untrusted source, remove secrets/network and apply OS-level CPU, memory, process, disk, and wall-time limits.

## Included example

The complete generalized-Fibonacci project demonstrates CSV-backed sequences, companion matrices, characteristic roots, a repeated-root edge case, 2D camera movement, 3D OpenGL, narration/captions, themes, profiles, validation, accessibility metadata, and expected deliverables:

```bash
manim-director --project examples/generalized-fibonacci \
  render --scene GeneralizedFibonacci --profile preview
```

Direct Manim equivalents remain available:

```bash
cd examples/generalized-fibonacci
manim -pql scenes.py GeneralizedFibonacci
manim -pql scenes.py CharacteristicRoots
manim -pql --renderer opengl scenes.py StateOrbit3D
```

The edit/debug fixture under `examples/fixtures/edit-debug` contains a deterministic editable scene, an intentional missing-asset failure, and the expected scoped repair.
