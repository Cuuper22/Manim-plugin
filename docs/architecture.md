# Architecture

One Rust binary coordinates; Python does only what needs Python. Listing a project, opening the
workbench, following a job or returning a cached render never imports Manim.

```text
 CLI ─┐
 MCP ─┼─> engine (Rust) ──> state.db (SQLite, WAL)
 HTTP ┘      │  ▲               jobs, logs, cache, leases
  ▲          │  │ JSON Lines
  │ SSE      ▼  │
workbench   runtime (Python) ──> Manim, FFmpeg, TeX
(React)       one process per request
```

## Pieces

| Path | Role |
|---|---|
| `crates/manim-director-core` | Types shared by every frontend: the operation catalog, one params struct per operation, tasks, results, errors, job records, the `director.yaml` model with profile resolution, and the file-type and ignore lists. No I/O beyond reading the spec. |
| `crates/manim-director-engine` | The scheduler, the bridge to Python, the SQLite store, the cache, the source editor, the HTTP server with its event stream, and the MCP server. |
| `crates/manim-director-cli` | The `manim-director` binary. |
| `runtime/src/manim_director_runtime` | The bridge worker (one handler module per operation) and the authoring API (`DirectedScene`, themes, layout, beats, derivations), plus packaged themes and templates. |
| `workbench/` | The React workbench, built once and embedded in the binary. |
| `skills/manim-director` | The agent skill; `scripts/mcp_launcher.py` starts the MCP server for Claude Code. |

## A job, end to end

1. **Request.** The CLI, MCP and HTTP each parse their input into the same typed `OperationRequest`
   (`{"operation": "render", "scene": …}`) through one module, so a parameter means the same thing
   everywhere.
2. **Resolve.** At submit the engine loads `director.yaml` once, resolves the scene and file, the
   profile (to exact width, height, fps, renderer and format), the media source and every path, and
   turns the request into a task. Invalid input fails here, before anything runs.
3. **Cache.** For `render` and `still`, a fingerprint of the task, the engine and runtime versions
   and the content of the project files a render can read either returns an earlier result or joins
   an identical running job.
4. **Queue.** The job row is written to `state.db` with its owner, the engine instance. Each engine
   heartbeats a lease; when one disappears, any other engine fails its unfinished jobs as
   `engine_lost`. CLI commands, `serve` and `mcp` can work on one project at the same time, and
   renders of the same scene take a lock so they run one at a time.
5. **Run.** A worker takes a Python process that is already started, with Manim imported (`serve` and
   `mcp` keep one ready), writes one request line, and reads progress, log and result frames until
   the process exits. Cancelling kills its process group.
6. **Validate.** The engine checks every artifact the runtime reports: inside the job's directory,
   non-empty, the right file signature, and for media, `ffprobe` against the requested profile.
7. **Publish.** The finished record is committed, then announced: the CLI prints it, MCP returns it,
   and the HTTP event stream sends `job` and `workspace` events, including for jobs other processes
   ran.

## Why it is shaped this way

- **One process per request** keeps Manim's global state, leaked updaters and GL contexts from
  bleeding between renders, and makes cancellation exact. Pre-warming takes the import cost (about a
  second) off the critical path.
- **Typed seams.** Every boundary (frontends to core, engine to runtime, server to workbench) is a
  typed struct on both sides with strict parsing, so a mistake fails with a field name instead of
  surfacing later as a wrong render.
- **Files stay on disk.** Results carry paths and sizes, never media bytes. Agents and the workbench
  fetch what they need, which keeps MCP answers small.
- **Plain Manim underneath.** `DirectedScene` is a mixin over Manim's scene classes and returns
  ordinary mobjects, so scenes render with `manim` alone and can use any Manim feature.
