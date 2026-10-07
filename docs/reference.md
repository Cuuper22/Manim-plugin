# Reference

Manim Director 2.0. Everything here is read from the code; when the two disagree, the code wins and
this page is the bug.

## CLI

```text
manim-director [--project PATH] [--json] <command>
```

`--project` is the project directory or any path inside it (default `.`); the engine walks up to the
nearest `director.yaml`. Path arguments are relative to your current directory and must stay inside
the project. Operation commands run one job, print progress on stderr and a summary with
project-relative artifact paths on stdout; `--json` prints the job record instead (`id`, `operation`,
`status`, `cached`, `request`, `result`, `error`, `progress`, timestamps, ...). Ctrl-C cancels the job.

| Command | Arguments |
|---|---|
| `init [PATH]` | `--name N` `--template T` `--theme T` `--seed N` `--force`, or `--scene-template T` to add a scene to the project |
| `doctor` | |
| `render` | `--scene S` `--file F` `--profile P` `--sections` `--fresh` |
| `still` | `--scene S` `--file F` `--profile P` `--fresh` |
| `frame` | `--at SECONDS` and a source |
| `contact-sheet` | a source, `--count N` (6, 1–24) `--columns N` (3, 1–8) |
| `qa` | a source, `--frames N` (8, 1–40) |
| `diagnose` | `--job ID`, `--text TEXT` or `--text-file PATH` |
| `validate-math STEP STEP...` | `--range VAR=LO:HI`... `--samples N` (200) `--tolerance X` (1e-9) `--seed N` |
| `captions PATH` | `--shift SECONDS` `--scale K` `--output PATH` (`.vtt` or `.srt`) |
| `ingest PATH...` | `--id ID`... (paired with the paths in order) `--license L` `--attribution A` `--normalize` `--force` |
| `export` | `--format zip\|mp4\|webm\|gif` (zip), a source, `--output PATH` `--gif-fps N` `--gif-width N` |
| `inspect` | Scenes, profiles, latest renders and recent jobs |
| `edit PATH` | `--content S`, `--content-file F`, `--line START:END` with `--replacement S` or `--replacement-file F`, or `--merge-patch JSON` / `--merge-patch-file F` (only `director.yaml`); `--expected-revision R` |
| `serve` | `--host` (127.0.0.1) `--port` (4177, 0 = any) `--workbench-dir DIR` `--allow-remote` |
| `open` | as `serve`, plus `--no-browser` |
| `mcp` | MCP over stdio |

A *source* is `--job ID` (a succeeded job's video, or for `qa` its image), `--path FILE`, or by
default the latest successful render of `--scene` (default `engine.main_scene`), filtered by
`--profile`.

Exit codes: `0` succeeded; `1` the job failed or was cancelled (including timeouts); `2` invalid
input; `3` the runtime is unavailable, the engine was lost, or an internal error; `130` interrupted.
A `validate-math` that finds a wrong step and a `qa` that finds problems both succeed: read the
verdict.

## Environment

| Variable | Effect |
|---|---|
| `MANIM_DIRECTOR_PYTHON` | Interpreter for the runtime. Default: `<prefix>/share/manim-director/venv` beside the binary, else `python3` (`python` on Windows). |
| `MANIM_DIRECTOR_WORKERS` | Jobs run at once per engine process (2, 1–32). |
| `MANIM_DIRECTOR_QUEUE` | Queued jobs per engine process (128, 1–4096); beyond it, `queue_full`. |
| `MANIM_DIRECTOR_TIMEOUT_SECONDS` | Job timeout, over `budgets.render_seconds` (1800, 10–86400). |
| `MANIM_DIRECTOR_MEMORY_MB` | Address-space limit per runtime process on Unix, over `budgets.memory_mb`. Off by default: it can break NumPy, Cairo and OpenGL. |
| `MANIM_DIRECTOR_PREWARM` | `0` stops `serve` and `mcp` from keeping a runtime process warm. |
| `MANIM_DIRECTOR_KEEP_JOBS`, `MANIM_DIRECTOR_KEEP_DAYS` | Pruning of finished jobs and their artifacts (500 jobs, 30 days); the latest artifacts of every scene are kept. |
| `MANIM_DIRECTOR_WORKBENCH` | Same as `--workbench-dir`. |
| `MANIM_DIRECTOR_PREFIX` | Install prefix for `install.py`, and a place the MCP launcher looks for `bin/manim-director`. |
| `MANIM_DIRECTOR_RELEASE_BASE` | Where `install.py` downloads release archives and `SHA256SUMS` from: an `https://` or `file://` URL. |
| `RUST_LOG` | Engine log filter (default `warn`). |

## director.yaml

Only `project.name` is required. Keys not listed here are ignored by the engine.

```yaml
version: 1                       # the only version
project:
  name: Generalized Fibonacci
  title: …                       # optional display strings
  description: …
  seed: 73
  source_dir: scenes             # where scenes are found ("." for the root)
  asset_dir: assets
  output_dir: output             # exports land here
  media_dir: .manim-director/media
engine:
  source: scenes/main.py         # the main scene's file
  main_scene: GeneralizedFibonacci
theme: midnight                  # midnight | paper | chalkboard | contrast
direction:
  symbols: {p: primary, '\lambda': accent}   # TeX token -> theme token or #RRGGBB (read by the runtime)
safe_area: {top: 0.05, right: 0.05, bottom: 0.08, left: 0.05}   # fractions of the frame, each 0–0.45
render:
  profile: preview               # default profile
  renderer: cairo                # cairo | opengl
  format: mp4                    # mp4 | mov | webm | gif
  transparent: false
  width: 1920                    # the `custom` profile
  height: 1080
  fps: 60
profiles:                        # override a built-in or add one
  loop-gif: {resolution: [640, 360], fps: 15, format: gif}
  transparent: {quality: high, format: mov, alpha: true}
scenes:                          # optional ids for scenes
  - {id: narrative, class: GeneralizedFibonacci, file: scenes/main.py, purpose: …, duration_seconds: 38.1}
storyboard:
  - id: hook                     # matches a beat id in the scene
    intent: introduce            # introduce | explain | compare | reveal | prove | recap
    transition: continue         # continue | contrast | reveal | chapter
    audience_question: …
    takeaway: …
    focus: …
    visual_metaphor: …
    duration: 4.3
budgets: {render_seconds: 1800, output_mb: 2048, memory_mb: null}
brief: {duration_seconds: 38.1}
inputs: {data: [data/sequences.csv], sources: [{path: sources/notes.md}]}
captions: {source: captions.vtt}
narration: {manifest: narration.json, source: narration.md}
```

Built-in profiles: `draft` 854×480@15, `preview` 1280×720@30, `production` 1920×1080@60, `ultra`
3840×2160@60, `custom` from `render`. A profile entry takes `quality` (`low`, `medium`, `high`,
`production`, `fourk`), `resolution`, `fps`, `renderer`, `format` and `alpha`; width and height must
be even and 16–8192, fps 1–240, and `alpha` needs `mov` or `webm`. Directories must stay inside the
project. An invalid file fails every operation that needs it with `invalid_spec` and the YAML line.

## MCP tools

`manim-director mcp` speaks MCP 2025-06-18 over stdio for the project it was started in. Arguments
are the operation's parameters (the CLI flags in snake case: `at_seconds`, `shift_seconds`, ...);
sources are `{"job_id": "…"}` or `{"path": "…"}`.

| Tool | Arguments |
|---|---|
| `init` | `name`, `template`, `scene_template`, `theme`, `seed`, `force` |
| `inspect` | none |
| `doctor` | `wait_seconds` |
| `render` | `scene`, `file`, `profile`, `sections`, `fresh`, `wait_seconds` |
| `still` | `scene`, `file`, `profile`, `fresh`, `wait_seconds` |
| `contact_sheet` | `source`, `scene`, `profile`, `count`, `columns`, `wait_seconds` |
| `qa` | `source`, `scene`, `profile`, `frames`, `wait_seconds` |
| `validate_math` | `steps` (2–32), `ranges` (`{"x": [-3, 3]}`), `samples`, `tolerance`, `seed`, `wait_seconds` |
| `submit` | `operation` (any job operation) plus its parameters, `wait_seconds` |
| `job_status` | `job_id`, `cancel`, `cursor`, `limit` (20, 1–100), `wait_seconds` |

Job tools wait `wait_seconds` (20, 0–50) and answer `{job, result, error}`; `job_status` adds log
`events` and a `next_cursor`. The text content is a one-line summary followed by the absolute path of
every artifact. A failed or cancelled job, and any error before the job starts, is a result with
`isError: true` and `{error: {code, message, data}}`; only malformed JSON-RPC and unknown tools are
protocol errors. Structured content stays under 48 KiB (`inspect` under 32 KiB); longer lists are
cut and marked `truncated`.

## HTTP API

`serve` and `open` print `Workbench: http://127.0.0.1:<port>/?token=<token>`; with `--json`, stdout
also gets `{"event":"listening","url":…,"address":…}`. The token is random per process.

- **Auth.** Every `/api` request needs `Authorization: Bearer <token>` or the `mdsess_<port>`
  cookie, which a browser gets by opening the printed link (a `303` to the same path that sets an
  HttpOnly, SameSite=Strict cookie). Otherwise `401`.
- **Host and Origin.** The `Host` header must be `127.0.0.1`, `localhost` or `[::1]` with the bound
  port (`403 forbidden_host`) unless `--allow-remote`. Every POST and PUT needs
  `Content-Type: application/json` (`415`); one that carries an `Origin` must come from that same
  host (`403 forbidden_origin`). No CORS headers are ever sent.
- **Errors.** Every error body is `{"error": {"code", "message", "data"}}`; branch on `code`.

| Route | Purpose |
|---|---|
| `GET /api/health` | `{ok, version, api_version: 2, instance_id}` |
| `GET /api/state` | The workspace: engine info, project, spec, profiles, themes, scene index, scenes, storyboard, latest artifacts per scene, findings, doctor, the 50 newest jobs and an `event_cursor` |
| `POST /api/jobs` | Submit `{"operation": "render", "scene": "…", …}`; `202` queued, `200` cached or joined. `init`, `discover` and `ingest` are refused (`operation_not_allowed`). |
| `GET /api/jobs?before=&limit=` | Newest first (50, at most 200) |
| `GET /api/jobs/{id}` | One job with its result and artifact URLs |
| `POST /api/jobs/{id}/cancel` | Body `{}`; `202` while cancelling, `200` if it already ended |
| `GET /api/jobs/{id}/logs?after=&limit=` | Log records (200, at most 500) |
| `GET /api/source?path=&start_line=&end_line=` | A page of a text file (400 lines by default, at most 2000) with its `revision` |
| `PUT /api/source` | `{path, expected_revision, edit}`, where `edit` is `{"kind": "replace_all", content}`, `{"kind": "replace_lines", start_line, end_line, replacement}` or `{"kind": "merge_patch", patch}` (`director.yaml` only) |
| `GET`/`HEAD /api/files/{path}` | Stream a project file with `Range` support |
| `GET /api/events` | Server-sent events |

**Source writes.** `expected_revision` is the file's BLAKE3 from the last read, or `null` to create
it; anything else is `409 revision_conflict` with the current revision. Writes are atomic, keep the
file's line endings, and are validated first (Python syntax, JSON, `director.yaml`) with
`400 source_invalid` and the line. Editable types: `py json yaml yml toml md tex typ vtt srt txt`, up
to 2 MiB; hidden paths are refused. The previous content is kept under `.manim-director/undo/` (20
per file).

**Files.** Paths are project-relative. Hidden paths are refused except `.manim-director/artifacts/`;
the media directory is refused. Artifact URLs carry `?v=<size>-<mtime>`, and a file that changed since
answers `410 artifact_changed`. `?download=1` asks for an attachment.

**Events.** Each event has `id: <instance_id>.<seq>` and one of these types: `job` (every status
change of every job, including those from the CLI and MCP), `progress` (at most 4 per second per
job), `workspace` (complete replacement sections of the state), `file` (a scene file or
`director.yaml` changed on disk) and `resync`. Reconnect with `Last-Event-ID` or
`?after=<event_cursor>` to replay missed events; when they are gone (`expired`, `lagged`) or the
engine restarted (`unknown_cursor`), `resync` says to reload `/api/state`. At most 32 streams.

Limits: request bodies 256 KiB (`PUT /api/source` 3 MiB), served files 8 GiB.

## Bridge protocol

The engine runs the Python runtime as
`<python> -P -m manim_director_runtime bridge [--preload]` in the project root. One process serves
one request, then exits; `serve` and `mcp` keep one `--preload` process (Manim already imported)
waiting for the next job.

1. The runtime writes a `ready` frame: `{"type":"ready","protocol":2,"runtime_version","python",
   "manim","preloaded","preload_failed","preload_ms","catalog":{"themes","project_templates",
   "scene_templates"}}`.
2. The engine writes one request line and closes stdin:
   `{"protocol":2,"request_id":"<uuid>","method":"render","project_root":"/abs","params":{…task…}}`.
3. The runtime writes `progress` (`phase`, `current`, `total`, `scene_seconds`, `message`) and `log`
   (`level`, `message`) frames, then exactly one `result` (`{"result": {…}}`) or `error`
   (`{"error": {code, message, data}}`) frame, each carrying the `request_id`, and exits.

Frames are JSON Lines on the original stdout, each at most 1 MiB; everything else the scene prints
goes to stderr and into the job log. The request line is at most 4 MiB. The engine waits 120 s for
`ready`, then fails the job `runtime_unavailable`. A missing terminal frame is `runtime_crashed`; a
malformed frame, a wrong `request_id` or another protocol version is `runtime_protocol`. Cancelling
or timing out sends SIGTERM to the process group, then SIGKILL after 2 s.

Methods: `init`, `discover`, `doctor`, `render`, `still`, `frame`, `contact_sheet`, `qa`,
`diagnose`, `validate_math`, `captions`, `ingest`, `export`. `init` and `discover` run directly,
without a job; the others are jobs.

## Error codes

Before a job starts (HTTP status in brackets): `invalid_params` (400; `data.field`, `reason`,
sometimes `allowed`), `invalid_spec` (400; `line`, `column`), `operation_not_allowed` (400),
`not_found` (404), `source_not_found` (404; nothing rendered yet for that scene and profile),
`project_not_empty` (409), `request_too_large` and `budget_exceeded` (413), `queue_full` (429),
`internal` (500). The source and file routes add `invalid_path`, `unsupported_file_type`, `not_utf8`,
`file_too_large`, `line_out_of_range`, `source_invalid`, `revision_conflict`, `artifact_changed` and
`range_not_satisfiable`. The HTTP server adds `unauthorized` (401), `forbidden_host` and
`forbidden_origin` (403), `method_not_allowed` (405; `data.allowed`), `unsupported_media_type` (415)
and `too_many_streams` (429).

A failed job's `error.code`: `cancelled`, `timeout`, `engine_lost`, `runtime_unavailable`,
`runtime_protocol`, `runtime_crashed`, `artifact_invalid`, `budget_exceeded`, `dependency_missing`,
`scene_not_found`, `scene_ambiguous`, `scene_required`, `render_failed` (with `findings`,
`exception` and `traceback`), `media_error`, `invalid_source`, `invalid_expression`,
`invalid_captions`, `io_error`, `invalid_params`, `internal`.

Findings (`qa`, `doctor`, `diagnose`, render failures) are
`{code, severity, message, hint, location: {file, line}, at_seconds, beat, frame}`, with fields left
`null` when they do not apply.
