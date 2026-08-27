# Plugin runtime contract

This directory documents how the Codex-facing plugin layer attaches to the local Manim Director engine. User workflows and complete CLI examples belong in the main documentation; the contract here is intentionally small and stable.

## Package entrypoints

| Component | Declaration | Runtime |
|---|---|---|
| Plugin metadata | `.codex-plugin/plugin.json` | `manim-plugin`, displayed as **Manim Director** |
| Orchestration skill | `skills/manim-director/SKILL.md` | `$manim-director`, implicit invocation enabled |
| Agent tools | `.mcp.json` | `manim-director mcp` over stdio |
| Workbench | CLI | `manim-director serve --port 4177` |

The MCP declaration intentionally omits `cwd`, so the host workspace is inherited. The binary must be on `PATH`. A source installation builds the Rust workspace and workbench, installs the Python bridge, and installs the binary:

```bash
python3 scripts/install.py --with-manim
manim-director doctor
```

The default path downloads the platform release binary with the workbench embedded. `--from-source` builds the workbench and Rust binary locally. `--prefix` controls the binary and isolated Python environment together; its `bin` directory must be on `PATH` before the plugin starts. The engine resolves `<prefix>/share/manim-director/venv` relative to its executable unless `MANIM_DIRECTOR_PYTHON` explicitly overrides it.

## Why there is no `.app.json`

`.app.json` declares installed ChatGPT connector IDs. The workbench is a project-local web application served by the Rust process, not an external connector. Advertising a fabricated connector would make installation unreliable, so the plugin exposes the workbench through the documented `serve` command and MCP job resources instead.

## MCP surface

The server exposes ten coarse tools:

- `project_init`
- `project_inspect`
- `project_apply`
- `doctor`
- `render`
- `preview`
- `qa`
- `debug`
- `export`
- `job_status`

Detailed state is discoverable through `manim://project/spec`, `manim://jobs/recent`, and `manim://jobs/<uuid>`. Job-producing calls return a compact summary plus job ID and resource URI. Progress/log consumption is cursor-based and bounded; media and source files are referenced by path instead of embedded in tool responses.

This small surface is deliberate: animation authoring remains ordinary workspace editing, while the engine owns discovery, dependency checks, job scheduling, caching, rendering, QA, diagnosis, export, and artifact indexing.

## Project boundary

`director.yaml` marks the project root. Relative source, asset, media, and output paths must remain inside it. Runtime state lives in `.manim-director/state.db`; generated Manim media and temporary files live below `.manim-director/` unless the spec overrides them with another project-relative path.

The scheduler fingerprints source, configuration, assets, parameters, runtime versions, and deterministic seed. A matching successful result can be reused without replaying Manim. SQLite stores job summaries and ordered cursor events; large logs, frames, and videos remain files.

## Completion boundary

The skill requires a targeted preview and rendered-frame inspection after any source-affecting create, edit, repair, or migration. Production completion requires a readable requested artifact, metadata matching the output profile, and the configured QA evidence. Process success without media is a failed job.
