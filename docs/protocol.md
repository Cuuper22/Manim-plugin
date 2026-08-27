# Protocol contracts

Manim Director has four boundaries:

1. the internal Rust job model;
2. Rust-to-Python JSONL over standard I/O;
3. localhost REST/SSE for the workbench and integrations; and
4. MCP JSON-RPC for agents.

All contracts use UTF-8 JSON, snake-case enum values, UUID job IDs, and RFC 3339 UTC timestamps. Media and archives are referenced by path or URL; they are never embedded as base64 JSON.

## Common types

### Error

```json
{
  "code": "missing_dependency",
  "message": "Manim is not available to the selected Python interpreter",
  "data": {
    "python": "/usr/bin/python3"
  }
}
```

`code` is stable and machine-readable. `message` is concise and human-readable. `data` is optional structured evidence. Clients must branch on `code`, not parse `message`.

### Operation

Public operation values are:

```text
scaffold ingest discover inspect doctor render preview still contact_sheet
qa debug validate_math captions assets export sample
```

The Python method differs in two cases:

| Operation | Runtime method |
|---|---|
| `debug` | `diagnose` |
| `validate_math` | `math_validate` |

`init` is accepted as a CLI/parse alias for `scaffold`; `diagnose` is accepted for `debug`; `math_validate` is accepted for `validate_math`.

The direct bridge additionally implements `section`, `media`, `themes`, `templates`, and `capabilities`. These are runtime composition/discovery methods rather than first-class Rust `Operation` variants; public callers normally reach their behavior through render parameters, higher-level operations, or a capability query to the bridge.

### Job

```json
{
  "id": "d5f8a292-cb9b-495b-a851-4cb2c9def9f3",
  "sequence": 42,
  "project_root": "/work/fibonacci",
  "operation": "render",
  "status": "running",
  "params": {
    "scene": "GeneralizedFibonacciScene",
    "profile": "production"
  },
  "fingerprint": "81c7…",
  "result": null,
  "error": null,
  "created_at": "2026-08-27T02:12:30.402Z",
  "started_at": "2026-08-27T02:12:30.419Z",
  "finished_at": null,
  "cached": false
}
```

Status is one of `queued`, `running`, `succeeded`, `failed`, or `cancelled`. Terminal jobs have `finished_at` and either `result` or `error`. A cache hit is represented as a new `succeeded` job with `cached: true` and all three timestamps set.

### Cursor page

```json
{
  "items": [],
  "next_cursor": null
}
```

`next_cursor` is opaque to clients even where the current implementation encodes an integer. Omit it or send an empty value for the first page. Continue only when it is non-null.

## Python bridge: JSONL v1

The process is started as:

```bash
python -m manim_director_runtime bridge
```

Standard input and output contain one compact JSON object per line. Standard output is protocol-only. Human diagnostics belong on standard error and are converted by Rust into bounded `runtime_stderr` progress events.

The Rust engine currently creates one bridge process per job, writes one request, closes standard input, consumes messages, and waits for process exit. The framing still permits multiple sequential requests for direct runtime use, but callers must not interleave messages for one request out of order.

### Request

```json
{"request_id":"d5f8a292-cb9b-495b-a851-4cb2c9def9f3","method":"preview","params":{"project_root":"/work/fibonacci","scene":"GeneralizedFibonacciScene","profile":"draft"}}
```

| Field | Type | Rule |
|---|---|---|
| `request_id` | string | Required; returned unchanged on every response line. |
| `method` | string | Required canonical runtime method. |
| `params` | object | Required. Rust inserts canonical `project_root`, replacing a caller-supplied value. |

If internal job parameters are `null`, Rust sends an empty object plus `project_root`. If a non-object is supplied, it is preserved under `params.input`.

### Progress event

Zero or more events may precede the terminal response:

```json
{"request_id":"d5f8a292-cb9b-495b-a851-4cb2c9def9f3","type":"event","event":"render_progress","data":{"frame":180,"total_frames":600,"fraction":0.3}}
```

Event names are operation-defined. `data` defaults to JSON `null` when omitted. Consumers should display or persist unknown event names rather than failing the request.

### Success

Exactly one successful terminal line:

```json
{"request_id":"d5f8a292-cb9b-495b-a851-4cb2c9def9f3","type":"result","result":{"artifacts":[{"kind":"video","path":"output/fibonacci.mp4"}]}}
```

### Failure

Exactly one failed terminal line:

```json
{"request_id":"d5f8a292-cb9b-495b-a851-4cb2c9def9f3","type":"error","error":{"code":"scene_not_found","message":"Scene GeneralizedFibonaciScene was not found","data":{"available":["GeneralizedFibonacciScene"]}}}
```

After `result`, the process must exit successfully. A result followed by a non-zero exit is a transport failure. An `error` is authoritative even when the process exits non-zero. Exit without a terminal line becomes `runtime_transport`. A mismatched `request_id` or malformed stdout line is also a transport failure.

Cancellation terminates the bridge process and produces job error code `cancelled`. Timeout produces `job_timeout` with `data.timeout_seconds`.

## REST API v1

The default origin is `http://127.0.0.1:4177`. The server is project-scoped: one process serves one canonical project root. JSON request bodies are limited to 3 MiB; editable source content is independently capped at 2 MiB.

HTTP errors use this envelope:

```json
{
  "error": {
    "code": "bad_request",
    "message": "unsupported operation: frobnicate"
  }
}
```

The current HTTP mapping is:

| Status | Meaning |
|---:|---|
| `200` | Read succeeded, cancellation accepted, or submitted operation was served from cache. |
| `202` | A new operation was queued. |
| `400` | Malformed parameters, project mismatch, or invalid project. |
| `404` | Job/route/workbench was not found. |
| `413` | JSON body exceeded 3 MiB. |
| `500` | Internal persistence, queue, bridge, or serialization failure. |

### `GET /api/health`

Response:

```json
{"ok":true,"version":"1.0.0"}
```

This checks the Rust service, not optional Python/system dependencies. Use a `doctor` job for those.

### `GET /api/state`

Optional query: `project=<path>`. If supplied, it must canonicalize to the server's project root.

Response:

```json
{
  "project_root": "/work/fibonacci",
  "spec": {"version":1,"project":{"name":"Fibonacci"}},
  "files": {
    "source_count": 3,
    "asset_count": 8,
    "output_count": 2,
    "sources": ["scenes/main.py"],
    "assets": ["assets/spiral.svg"]
  },
  "jobs": {
    "items": [],
    "next_cursor": null
  }
}
```

`sources` and `assets` are sorted relative paths capped at 200 each; the counts describe the complete inventory. `jobs.items` contains up to 50 newest jobs.

### `GET /api/state/source`

Reads a bounded UTF-8 source span. Query parameters:

| Parameter | Type | Default | Rule |
|---|---|---:|---|
| `path` | string | required | Project-relative approved source/text path. |
| `start_line` | integer | `1` | 1-based first line. |
| `end_line` | integer | `start_line + 399` | Inclusive; a response is capped at 2000 lines. |

```json
{
  "path": "scenes/main.py",
  "revision": "763a5f…",
  "language": "python",
  "start_line": 20,
  "end_line": 42,
  "total_lines": 186,
  "content": "class Roots(Scene):\n    ..."
}
```

The revision is a BLAKE3 digest of the complete file, not only the returned span. Files larger than 2 MiB are rejected by the source layer.

### `PUT /api/state/source`

Applies exactly one mutation mode:

**Full replacement**

```json
{
  "path": "scenes/main.py",
  "content": "from manim import *\n...",
  "expected_revision": "763a5f…"
}
```

**Inclusive line replacement**

```json
{
  "path": "scenes/main.py",
  "start_line": 40,
  "end_line": 43,
  "replacement": "        self.wait(2)",
  "expected_revision": "763a5f…"
}
```

**JSON Merge Patch of `director.yaml`**

```json
{
  "path": "director.yaml",
  "merge_patch": {"render":{"profile":"production"}},
  "expected_revision": "c82417…"
}
```

`content`, line-edit fields, and `merge_patch` are mutually exclusive. `merge_patch` is valid only for `director.yaml`. A line edit uses 1-based inclusive bounds; an empty replacement deletes the range. `expected_revision` is optional but strongly recommended for any interactive/agent write.

The server validates Python syntax, project-spec YAML, or JSON as applicable, snapshots the previous file, then atomically renames the new file into place. Response:

```json
{
  "path": "scenes/main.py",
  "previous_revision": "763a5f…",
  "revision": "48de12…",
  "bytes": 4518,
  "undo_path": ".manim-director/undo/20260827T021230.000Z-…/scenes/main.py",
  "affected_scenes": ["roots"]
}
```

Approved extensions are `.py`, `.json`, `.yaml`, `.yml`, `.toml`, `.md`, `.tex`, `.typ`, `.vtt`, `.srt`, and `.txt`, plus the root `director.yaml`. `.manim-director` cannot be edited through this API. Source reads and replacements are capped at 2 MiB; the 3 MiB JSON limit leaves bounded framing room for a complete replacement.

### `GET /api/files`

Downloads a finished project artifact. Query `path` is a project-relative file path:

```text
GET /api/files?path=output%2Frecurrence-film-source.zip
```

The server canonicalizes the path, rejects traversal/symlink escape, rejects state DB/undo/temp paths, requires a regular file no larger than 8 GiB, and allowlists common video, image, audio, caption, archive, source, data, and PDF extensions. The response streams the file with detected `Content-Type`, exact `Content-Length`, `Content-Disposition: attachment`, and `X-Content-Type-Options: nosniff`.

### `POST /api/intents`

Submits a high-level intent:

```json
{
  "intent": "preview the roots section",
  "operation": "preview",
  "params": {
    "scene": "GeneralizedFibonacciScene",
    "section": "roots"
  }
}
```

`intent` is required. `operation` is optional. If omitted, the compact classifier recognizes scaffold, ingest, export, debug, QA, preview, inspect, and render language; edit/migration language is routed to inspection so a deterministic local process never invents a source mutation, and unknown text also defaults safely to inspection. The exact intent is inserted into the runtime parameters. Explicit `operation` is preferred for programmatic clients. Free-form semantic edits are translated by Codex into revision-checked `project_apply` calls, not guessed by this endpoint.

Response is a `Job` with `202` or `200` when cached.

### `POST /api/ingest`

Imports one or more source materials into the project:

```json
{
  "paths": ["/incoming/notes.md", "/incoming/data.csv"],
  "params": {
    "destination_dir": "sources",
    "manifest": "sources/manifest.json",
    "summary_chars": 4000
  }
}
```

`paths` must be a non-empty string array. The runtime recognizes Markdown, LaTeX, Typst, CSV, JSON, Python, notebooks, SVG/raster images, audio, video, and—with the `ingest`/`full` Python extra—PDF. It copies inputs into a project-confined destination, produces bounded content/metadata summaries, and writes a provenance-capable manifest. Response is a `Job` with `202`; ingestion is intentionally not cacheable because it mutates project inputs.

### `POST /api/renders`

```json
{
  "scene": "GeneralizedFibonacciScene",
  "profile": "production",
  "section": "derivation",
  "params": {
    "renderer": "cairo",
    "format": "mp4"
  }
}
```

All fields are optional, but the runtime may reject an underspecified project. `scene`, `profile`, and `section` are merged into `params`. Response is a `Job` with `202` or `200` when cached.

### `POST /api/qa`

Runs visual and artifact QA against a completed render or explicit inputs:

```json
{
  "job_id": "d5f8a292-cb9b-495b-a851-4cb2c9def9f3",
  "scene": "GeneralizedFibonacciScene",
  "profile": "preview",
  "params": {"sample_frames": 8}
}
```

`job_id`, `scene`, `profile`, `source`, and `images` are optional and merged into `params`. When `job_id` is present, the scheduler hydrates the completed job's final artifact instead of copying media into the request. Response is a `Job` with `202`.

### `GET /api/renders/{id}`

Returns the complete current `Job`. The route is named for renders but accepts the UUID of any engine job.

### `POST /api/renders/{id}/cancel`

Requests cancellation and returns the current `Job` snapshot. Cancellation is asynchronous; use SSE or fetch the job until its status becomes `cancelled` or another terminal state.

### `POST /api/exports`

```json
{
  "format": "bundle",
  "output": "output/fibonacci-director.zip",
  "params": {
    "include_media": true
  }
}
```

`format` and `output` are merged into the operation parameters. Response is a `Job` with `202` or `200` when cached.

### `GET /api/logs`

Query parameters:

| Parameter | Type | Default | Meaning |
|---|---|---:|---|
| `job_id` | UUID | all jobs | Filter to one job. |
| `cursor` | integer token | `0` | Return records after this cursor. |
| `limit` | integer | `100` | Page size, clamped to 1–500. |

Each item is:

```json
{
  "cursor": 91,
  "job_id": "d5f8a292-cb9b-495b-a851-4cb2c9def9f3",
  "timestamp": "2026-08-27T02:12:34.000Z",
  "level": "info",
  "event": "render_progress",
  "data": {"fraction":0.3}
}
```

### `GET /api/events`

Returns `text/event-stream` with keep-alive frames. Named events are:

| SSE event | Data payload |
|---|---|
| `job_queued` | `{"type":"job_queued","job":JobSummary}` |
| `job_started` | `{"type":"job_started","job":JobSummary}` |
| `job_progress` | `{"type":"job_progress","job_id":UUID,"event":string,"data":any}` |
| `job_finished` | `{"type":"job_finished","job":JobSummary,"result":any?,"error":Error?}` |

SSE is a live invalidation/progress stream, not durable replay. If a client disconnects or falls behind, it reloads `/api/state`, fetches the relevant job, then continues logs from its last cursor.

## MCP server

Start the stdio server with:

```bash
manim-director mcp --project /work/fibonacci
```

The transport is newline-delimited JSON-RPC 2.0. It advertises MCP protocol version `2025-06-18`, tools, and non-subscribable resources. Notifications without an `id` receive no response.

### Tools

| Tool | Input | Compact result |
|---|---|---|
| `project_init` | `{name?: string, force?: boolean, seed?: 0..2147483647}` | Queued scaffold job and job resource. |
| `project_inspect` | `{}` | Project name, source/asset counts, spec resource. |
| `project_apply` | Source mutation fields, or `{ingest:[path,...]}` | Atomic edit result/undo path, or queued ingest job. |
| `doctor` | `{...}` | Queued environment check. |
| `render` | `{scene?, profile?, section?, ...}` | Queued/cached job. |
| `preview` | `{scene?, profile?, section?, ...}` | Queued/cached job. |
| `qa` | `{scene?, profile?, job_id?, source?, images?, ...}` | Queued artifact QA. |
| `debug` | `{scene?, profile?, job_id?, ...}` | Queued diagnosis. |
| `export` | `{format?, output?, job_id?, ...}` | Queued export. |
| `job_status` | `{job_id: UUID, cursor?: string, limit?: 1..100}` | Status, next bounded event page, and resource URIs. |

A submitted tool returns both short text and structured content:

```json
{
  "content": [{"type":"text","text":"queued preview d5f8…; manim://jobs/d5f8…"}],
  "structuredContent": {
    "job_id": "d5f8a292-cb9b-495b-a851-4cb2c9def9f3",
    "status": "queued",
    "cached": false,
    "resource": "manim://jobs/d5f8a292-cb9b-495b-a851-4cb2c9def9f3"
  },
  "isError": false
}
```

`job_status` defaults to 20 events, reports `next_cursor`, and links both the job and its log resource. The agent should continue from that cursor rather than replaying earlier logs, then read a resource only when it needs more detail.

### Resources

| URI | MIME | Contents |
|---|---|---|
| `manim://project/spec` | `text/yaml` | Exact `director.yaml` up to 128 KiB; oversized specs return `resource_too_large`. |
| `manim://jobs/recent` | `application/json` | Up to 50 compact job summaries. |
| `manim://jobs/{uuid}` | `application/json` | Compact job metadata/result (large values summarized) plus a paged log URI. |
| `manim://logs/{uuid}?cursor=N&limit=L` | `application/json` | Byte-budgeted log cursor page; `L` is clamped to 1–100. |

`resources/list` advertises the fixed project/recent resources plus up to 20 recent individual job resources.

### JSON-RPC errors

| Code | Meaning |
|---:|---|
| `-32700` | Parse error. |
| `-32601` | Method not found. |
| `-32602` | Invalid tool/resource arguments or unknown tool. |
| `-32004` | Job or resource not found. |
| `-32000` | Internal engine/store error. |

Runtime job failures are not JSON-RPC failures once submission succeeded. They are terminal job state and are read through `job_status` or `manim://jobs/{uuid}`.

## Compatibility rules

- New optional object fields may be added without a protocol-version bump; clients must ignore unknown fields.
- Existing enum values, field meaning, and error codes do not change within a major version.
- A new required field or changed semantic requires a new project/protocol version or a parallel endpoint/method.
- Bridge stdout remains JSONL-only. Any dependency that writes arbitrary stdout must be captured or redirected by the Python runtime.
- Paths in responses are filesystem paths for the server host. Remote clients must not assume they can open them directly.
