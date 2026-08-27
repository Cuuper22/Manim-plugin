# Behavior contract

These 24 feature files are the product acceptance contract. They cover the whole plugin, so a step may bind to one of four real boundaries: the Codex skill workflow, the Rust control plane, the thin Python runtime, or the React workbench. A scenario does not imply that the local binary invents creative source changes or translations on its own.

| Tag | Capability | Concrete implementation boundary |
|---|---|---|
| `@F01` | Request routing | Codex skill mode router; REST intent routing only queues explicit local operations and safely maps edit/migration language to inspect |
| `@F02` | Brief compilation | Codex skill plus persisted `brief`, render, and project defaults in `director.yaml` |
| `@F03` | Source ingestion | Runtime `ingest`, Rust `project_apply` ingestion, and Codex conflict reconciliation |
| `@F04` | Storyboard/pedagogy | Codex authoring workflow and typed storyboard/spec records; generalized-Fibonacci fixture |
| `@F05` | Environment diagnosis | Runtime `doctor` and diagnostic classifiers; fallbacks remain explicit rerender requests |
| `@F06` | Project scaffolding | Runtime `scaffold` and MCP `project_init`; CLI `init` is the compact local initializer |
| `@F07` | Code authoring | Codex authoring plus AST discovery, revision-checked edits, cache invalidation, and targeted preview |
| `@F08` | Manim vocabulary | Codex-authored ordinary Manim Python; templates and the 2D/3D example provide executable fixtures |
| `@F09` | Math/data correctness | Runtime `math_validate`, source-backed example data, and focused project assertions |
| `@F10` | Typography/formulas | Manim/Pango/LaTeX/Typst render path, doctor capability report, and compiler diagnostics |
| `@F11` | Animation direction | Codex direction rules plus timed storyboard, narration, and scene source |
| `@F12` | Assets/provenance | Runtime asset inventory/normalization, preserved manifest metadata, and render diagnostics |
| `@F13` | Narration/captions/audio | Runtime caption validation/generation/reconciliation and FFmpeg mix/normalize operations |
| `@F14` | Rendering/profiles | Rust profile hydration and artifact contract; runtime Manim renderer and media probe |
| `@F15` | Preview/contact sheet | Runtime section artifact selection and contact sheets; workbench caption overlay |
| `@F16` | Visual QA | Runtime frame metrics and optional object metadata; Codex-directed repair is capped at two passes |
| `@F17` | Debug/repair | Runtime diagnostics, revision-checked edits, targeted preview, and Codex repair workflow |
| `@F18` | Natural-language editing | Codex semantic translation into MCP `project_apply`; the local intent endpoint does not guess edits |
| `@F19` | Interactive workbench | React workbench over REST/SSE: project/source navigation, revision-safe save, jobs, logs, playback, and export |
| `@F20` | Templates/themes | Runtime JSONL scene templates/themes plus bundled storyboard recipes used by the Codex skill |
| `@F21` | Compatibility/integrations | Codex migration workflow using inspect/edit/render/QA; extension selection and compatibility stay explicit |
| `@F22` | Export | Runtime bundle, MP4, WebM, GIF, and caption-package exporters; Rust job-artifact hydration |
| `@F23` | Variants/accessibility | Explicit authored variants using profiles, themes, captions, responsive source, and Codex review |
| `@F24` | Reliability/security | Rust scheduler/cache/artifact checks and process-tree cancellation; trusted host execution with external isolation guidance |

The wording distinguishes implemented automation from authored workflow. In particular, Director does not claim automatic platform publishing, automatic localization/accessibility repair, transparent host sandboxing, or resumable partial Manim segments. It validates complete artifacts, reuses only complete cache entries, and reports when a human or Codex-authored source change is still required.

## Binding and structure

Scenarios use standard `Feature`, `Scenario`/`Scenario Outline`, `Given`, `When`, `Then`, `And`, `Examples`, and table syntax. A runner may bind steps across the boundaries above without changing the behavior language. Platform-dependent render examples must report an unavailable capability rather than silently pass.
