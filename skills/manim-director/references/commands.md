# Command and tool contract

Read only the part of this reference needed for the current operation. The CLI is convenient for humans and scripts; the MCP surface is optimized for bounded agent context. Both use the same scheduler, cache, Python bridge, and `.manim-director/state.db`.

## CLI shape

```text
manim-director [--project PATH] [--json] <command>
```

`--project` may name the project root or any path inside it; the engine searches upward for `director.yaml`. `--json` emits one machine-readable JSON value instead of the human-formatted result.

### Project and environment

```bash
manim-director init [PATH] [--name NAME] [--force]
manim-director inspect [--deep]
manim-director edit PATH --content TEXT [--expected-revision HASH]
manim-director edit PATH --content-file PATH [--expected-revision HASH]
manim-director edit PATH --line START:END \
  (--replacement TEXT | --replacement-file PATH) [--expected-revision HASH]
manim-director edit director.yaml \
  (--merge-patch JSON | --merge-patch-file PATH) [--expected-revision HASH]
manim-director ingest PATH... [--set KEY=JSON]...
manim-director doctor [--set KEY=JSON]...
```

- `init` creates the version-1 spec, source/assets/output/runtime directories, and a runnable `MainScene`. It refuses to replace generated files unless `--force` is explicit.
- `inspect` returns the parsed spec and bounded file inventory. `--deep` also asks the Python runtime to discover scene classes and capabilities.
- `edit` atomically replaces a small file, applies a line-range replacement, or JSON-merge-patches `director.yaml`. File forms keep large content out of the shell, and `--expected-revision` prevents concurrent overwrite.
- `ingest` copies supplied notes, code, documents, data, or media metadata into the project, creates bounded summaries, and records a source manifest. Use `--set destination_dir=...`, provenance, or explicit size budgets when needed.
- `doctor` checks the installed Python/Manim and relevant rendering, text, media, and visual dependencies.

### Render, preview, QA, and repair

```bash
manim-director render \
  [--scene CLASS] [--profile PROFILE] [--section NAME] \
  [--renderer cairo|opengl] [--transparent] [--set KEY=JSON]...

manim-director preview \
  [--scene CLASS] [--profile PROFILE] [--section NAME] \
  [--contact-sheet] [--set KEY=JSON]...

manim-director qa \
  [--scene CLASS] [--artifact PATH] [--set KEY=JSON]...

manim-director debug \
  [--scene CLASS] [--job-id UUID] [--set KEY=JSON]...
```

CLI jobs wait for completion and a cache hit returns immediately. Use the workbench/API or MCP tools when an asynchronous job ID and cursor polling are preferable.

Built-in render profiles are `draft` (854×480 at 15 FPS), `preview` (1280×720 at 30 FPS), `production` (1920×1080 at 60 FPS), and `ultra` (3840×2160 at 60 FPS). Use `--profile custom --set width=... --set height=... --set fps=...` for another target.

Common `--set` render keys include `scene_file`, `scenes`, `all_scenes`, `format`, `width`, `height`, `fps`, `media_dir`, `output_name`, `output`, `timeout`, `save_sections`, `disable_caching`, and `flush_cache`. Each `--set KEY=JSON` is parsed as JSON when possible and otherwise as a string. Use it for supported advanced parameters instead of inventing new CLI flags.

`--transparent` requires a full-alpha format such as MOV or WebM; MP4 and GIF are rejected for full-alpha output. `--section` targets the named Manim section and preserves reusable outputs for unaffected sections.

`preview --contact-sheet` adds representative frame evidence. `qa --artifact` accepts a project-relative image or video; video QA extracts representative frames. `debug --job-id` diagnoses the stored failure and its bounded logs. Use `--scene` to constrain either operation further.

### Export and workbench

```bash
manim-director export \
  [--format zip] [--output PATH] [--job-id UUID] [--set KEY=JSON]...

manim-director open \
  [--host ADDRESS] [--port PORT] [--workbench-dir PATH] [--no-browser]

manim-director serve \
  [--host ADDRESS] [--port PORT] [--workbench-dir PATH]

manim-director mcp
```

`export` packages the source/config/assets and selected job deliverables. The default is ZIP. `open` serves the API/workbench and opens the browser unless `--no-browser` is used; `serve` runs the same server without launching a browser. Both default to `127.0.0.1:4177`. `MANIM_DIRECTOR_WORKBENCH` or `--workbench-dir` overrides the bundled UI.

`mcp` is started by `.mcp.json` over stdio. Do not mix its stdout with application logging; structured logs go to job state/stderr.

## MCP tools

The server exposes exactly ten tools.

| Tool | Primary input | Result |
|---|---|---|
| `project_init` | `name?`, `force?` | scaffold job |
| `project_inspect` | none | compact name/counts and spec resource |
| `project_apply` | path plus one edit mode, or `ingest` paths | atomic edit result or ingest job |
| `doctor` | optional runtime parameters | dependency job |
| `render` | target/profile plus supported render parameters | production render job |
| `preview` | target/profile plus supported render parameters | low-latency preview job |
| `qa` | target/artifact plus QA parameters | QA job |
| `debug` | target/job/log parameters | diagnosis job |
| `export` | `format?`, `output?`, `job_id?` plus export parameters | export job |
| `job_status` | `job_id`, `cursor?`, `limit?` | status plus only newer bounded events |

### Applying edits

`project_apply` is confined to the project and supports exactly one mode per call:

```json
{"path":"scenes/main.py","start_line":40,"end_line":48,"replacement":"...","expected_revision":"..."}
```

```json
{"path":"director.yaml","merge_patch":{"render":{"profile":"production"}},"expected_revision":"..."}
```

```json
{"path":"scenes/new_scene.py","content":"..."}
```

Use a line edit for existing source, JSON merge patch for `director.yaml`, and full content only for a genuinely new or small file. `expected_revision` prevents overwriting a concurrent/manual edit. Success returns `previous_revision`, new `revision`, byte count, snapshot `undo_path`, and `affected_scenes`. Python edits are syntax-checked before the atomic replacement.

The same tool deliberately multiplexes source ingestion to keep the MCP surface small:

```json
{"ingest":["/authorized/input/notes.md","/authorized/input/data.csv"]}
```

Ingest returns a job. The runtime copies inputs into the project, extracts bounded type-appropriate summaries/metadata, and writes a manifest; it never returns the whole document or media payload.

### Jobs and cursors

Job-producing tools return a one-line summary plus structured content:

```json
{
  "job_id": "uuid",
  "status": "queued",
  "cached": false,
  "resource": "manim://jobs/uuid"
}
```

Poll with the last returned cursor:

```json
{"job_id":"uuid","cursor":"42","limit":20}
```

`job_status` returns `events`, `next_cursor`, the compact job resource, and a paginated logs resource. Reuse `next_cursor`; do not restart at zero. Limits are clamped to 1–100.

## MCP resources

- `manim://project/spec` — the project YAML.
- `manim://jobs/recent` — bounded recent job summaries.
- `manim://jobs/<uuid>` — one job record plus a logs URI, not inline logs.
- `manim://logs/<uuid>?cursor=<n>&limit=<n>` — a bounded cursor page.

Tool and resource results return paths and metadata for source, frames, reports, and media. Open the specific local file with the host's appropriate file/image/video viewer; never request binary media or entire source trees through MCP.

## Useful sequences

Create and inspect:

```bash
manim-director init my-film --name "My Film"
manim-director --project my-film inspect --deep
manim-director --project my-film doctor
```

Preview a changed section, inspect it, then render production:

```bash
manim-director preview --scene MainScene --contact-sheet --set output=output/preview.mp4
manim-director qa --artifact output/preview.mp4
manim-director render --scene MainScene --profile production --set output=output/final.mp4
```

Export a reproducible handoff from a successful job:

```bash
manim-director export --format zip --output output/project.zip --job-id 00000000-0000-0000-0000-000000000000
```

Replace the example UUID with the actual successful job ID.
