# Manim Director

Manim Director is a production-grade Codex plugin and local toolchain for creating, editing, rendering, debugging, and shipping [Manim Community](https://www.manim.community/) animations.

It pairs a fast Rust control plane with a thin Manim-native Python runtime and an embedded React workbench. Large logs, source trees, and media stay on disk; Codex receives compact results, cursors, and resource URIs instead of context-filling payloads.

## What ships

- A versioned `director.yaml` project format for scenes, storyboards, narration, themes, assets, profiles, variants, and deliverables.
- Create, inspect, ingest, edit, render, preview, QA, debug, export, and workbench flows from one CLI.
- Ten coarse MCP tools with bounded responses, paged job/log access, and no base64 media transport.
- Atomic source edits with revision checks, snapshots, undo metadata, syntax validation, and targeted cache invalidation.
- Source ingestion for Markdown, LaTeX, Typst, CSV, JSON, Python, notebooks, PDFs, images, SVG, audio, and video.
- Cairo and OpenGL render orchestration, custom profiles, section renders, stills, contact sheets, transparent output, and variants.
- Visual, mathematical, caption, artifact, and environment checks with surgical diagnostics.
- MP4, WebM, GIF, caption-package, and reproducible project-bundle exports.
- A responsive workbench with project/assets navigation, playback, timeline tracks, inspector, revision-safe code editing, operation intents, render queue, logs, and exports.
- A complete generalized-Fibonacci production example and 24 feature-level BDD specifications.

## Install

Python 3.11+ is required for the runtime. The default installer verifies the platform archive against the release's exact `SHA256SUMS` entry before extraction, then creates a prefix-local isolated Python environment; `--with-manim` installs the complete tested Manim dependency set into it. Linux binaries are static musl builds rather than artifacts tied to the release runner's glibc, and every archive includes the project license and third-party notices.

```bash
git clone https://github.com/Cuuper22/Manim-plugin.git
cd Manim-plugin
python3 scripts/install.py --with-manim
manim-director doctor
```

If a release binary is unavailable for the machine, the installer falls back to a local Rust/Node build when those toolchains are present. To request that path directly:

```bash
python3 scripts/install.py --from-source --with-manim
```

Then install the Codex plugin from its repository marketplace:

```text
codex plugin marketplace add Cuuper22/Manim-plugin
codex plugin add manim-plugin@manim-director
```

Start a new Codex session after installation. The `$manim-director` skill and local MCP server will be available to direct projects without pulling whole files or render output into context.

## First project

```bash
manim-director init my-animation --name "My animation"
cd my-animation
manim-director doctor
manim-director preview --scene MainScene
manim-director open
```

Try the included production example:

```bash
manim-director --project examples/generalized-fibonacci inspect --deep
manim-director --project examples/generalized-fibonacci render \
  --scene SequenceData --profile preview
manim-director --project examples/generalized-fibonacci qa \
  --scene SequenceData
```

The same source remains standard Manim code:

```bash
cd examples/generalized-fibonacci
manim -pql scenes.py GeneralizedFibonacci
```

## Fast without being thin

| Layer | Responsibility | Why it stays lean |
|---|---|---|
| Rust engine | CLI, jobs, SQLite state, cache, REST/SSE, MCP, process control, artifact streaming | One stripped binary; bounded queues, output, logs, and memory; content-addressed invalidation |
| Python runtime | Manim discovery/rendering, media, math, assets, captions, QA, export | Zero mandatory dependencies; expensive imports occur only for the requested operation |
| React workbench | Visual authoring and live job control | Compiled once and embedded in the Rust binary; no separate production server |

On Unix, the default worker address-space ceiling is 8 GiB and can be changed with `MANIM_DIRECTOR_MEMORY_MB`; Windows deployments should apply the equivalent Job Object or container limit. Concurrency, queue depth, timeouts, request bodies, log pages, artifact sizes, and source summaries are independently bounded. Cancellation terminates the complete Manim/FFmpeg process tree.

## Project file

```yaml
version: 1
project:
  name: recurrence-film
engine:
  source: scenes.py
  main_scene: MainScene
render:
  profile: preview
  renderer: cairo
storyboard:
  - id: hook
    scene: MainScene
    duration: 4.0
    intent: Establish the visual question
```

Built-in profiles remain low-cost unless explicitly overridden; custom profiles can set resolution, frame rate, renderer, format, and alpha. See [the user guide](docs/user-guide.md) and [project specification](skills/manim-director/references/project-spec.md) for the complete schema.

## Development

```bash
make check
make build
make dev
```

The focused checks are:

```bash
cargo test --workspace --locked
python3 -m pytest runtime/tests -q
npm --prefix workbench ci
npm --prefix workbench run build
```

Useful references:

- [Architecture](docs/architecture.md)
- [CLI, REST/SSE, JSONL, MCP, and resources](docs/protocol.md)
- [Performance and context budgets](docs/performance.md)
- [Security model](docs/security.md)
- [Plugin installation](docs/plugin/installing.md)
- [BDD feature map](features/README.md)

## Security boundary

Manim scenes are executable Python. Director confines its own paths, validates requests and artifacts, bounds worker resources, and avoids shell command construction, but it does not pretend arbitrary Python is a sandbox. Render unreviewed projects inside a container or VM without host secrets or network access; the exact operational guidance is in [docs/security.md](docs/security.md).

MIT © Cuuper22
