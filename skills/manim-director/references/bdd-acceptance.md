# BDD acceptance contract

Read this reference when defining acceptance criteria, auditing a release, or deciding whether a requested deliverable is complete.

The canonical scenarios are the 24 files under `../../../features/`, tagged `@F01` through `@F24`. Read `../../../features/README.md` first: its crosswalk identifies whether each behavior is enforced by the Codex skill, Rust control plane, Python runtime, React workbench, or a combination. Do not reinterpret a Codex-directed authoring behavior as autonomous local-engine automation.

## Acceptance rules

- Bind each scenario to observable inputs and outputs at the implementation boundary named in the crosswalk.
- Keep creative actions such as semantic source editing, migration, localization, and accessibility remediation explicit and source-backed.
- Do not claim automatic platform publishing, localization, accessibility repair, partial-render resume, or a host sandbox.
- A render passes only when the requested artifact exists, is readable, and matches its declared dimensions, frame rate, container, and alpha requirement.
- A cancelled or interrupted render may reuse only a separately completed, validated cache entry. Manim partial movie segments are never final artifacts.
- Visual QA may automate frame-wide signals and metadata-backed object checks. Human or Codex inspection remains required for meaning, pedagogy, continuity, and ambiguous repairs.
- Platform-dependent capabilities may be unavailable only when the result reports that capability explicitly; an unavailable path is never a silent pass.
- Context-facing results remain bounded: use job cursors, compact summaries, artifact paths, and resource URIs instead of full projects, full logs, frame sequences, or media blobs.

The fast unit and protocol suite runs on every change. Slow render fixtures should cover only representative paths needed to prove the requested release behavior.
