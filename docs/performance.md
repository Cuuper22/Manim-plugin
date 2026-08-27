# Performance and context budgets

Manim Director is optimized around one observation: rendering is necessarily heavy, directing it is not. The Rust control plane stays responsive and compact; Python and Manim exist only inside bounded operation processes; media is passed by path instead of being copied through JSON or model context.

## Shipped limits

These are runtime defaults or hard clamps in the engine, not suggested values:

| Resource | Default | Allowed range / cap | Configuration |
|---|---:|---:|---|
| Concurrent Python jobs | 2 | 1–32 | `MANIM_DIRECTOR_WORKERS` |
| Queued jobs | 128 | 1–4096 | `MANIM_DIRECTOR_QUEUE` |
| Per-job timeout | 3600 s | 10–86400 s | `MANIM_DIRECTOR_TIMEOUT_SECONDS` |
| Worker address space (Unix) | 8192 MiB | 128–262144 MiB | `MANIM_DIRECTOR_MEMORY_MB` |
| Engine broadcast backlog | 1024 events | fixed | internal |
| Job page | caller selected | 200 items max | cursor + `limit` |
| Log page | caller selected | 500 items max | cursor + `limit` |
| Persisted log event | — | 16 KiB encoded | compact truncation marker |
| Persisted logs per job | — | 2,000 events / 2 MiB | fixed |
| Persisted logs per project | — | 50,000 events / 64 MiB | oldest terminal-job logs pruned |
| SQLite lock wait | 5 s | fixed | internal |
| Runtime JSONL request / result line | — | 4 MiB | fixed |
| Runtime stderr event | — | 2000 UTF-8 bytes per line | internal truncation |
| REST JSON request | — | 3 MiB | fixed; source content remains capped at 2 MiB |
| Cache hashing buffer | 64 KiB | fixed | internal |

Values outside an environment variable's allowed range are clamped, so a typo cannot create an unbounded worker pool, queue, timeout, or Unix address-space ceiling. Windows does not expose the Unix `RLIMIT_AS` primitive; apply the equivalent Job Object or container limit there.

Two workers is intentional. Cairo and FFmpeg jobs can saturate CPU and memory by themselves; more orchestration concurrency does not mean more render throughput. Increase it only when jobs are small or the machine has enough independent capacity:

```bash
MANIM_DIRECTOR_WORKERS=4 manim-director serve
```

For a memory-constrained laptop or CI runner:

```bash
MANIM_DIRECTOR_WORKERS=1 MANIM_DIRECTOR_QUEUE=32 manim-director serve
```

## Process and memory model

The server does not import Python libraries. For each started job, it launches:

```text
python -m manim_director_runtime bridge
```

The process accepts that job's one request, emits events and one terminal message, and exits. This costs process startup time but provides three useful bounds:

- Manim, NumPy, Pillow, SymPy, TeX, and renderer allocations are released after every operation.
- One plugin or scene cannot leak Python state into the next job.
- Cancellation and timeout can terminate the isolated bridge at a clear process boundary.

The runtime imports operation modules lazily. `doctor`, `inspect`, or `discover` does not import Manim; media and math dependencies load only in methods that use them.

The compiled workbench is static. Production serving does not keep Vite, Node, or a server-side JavaScript runtime resident.

## Cache behavior

Cacheable operations are keyed with BLAKE3 over the operation, canonical JSON parameters, and relevant project inputs. Hashing streams files in 64 KiB chunks; it does not read a video-sized project into memory.

The cache includes files with these relevant extensions:

```text
.py .svg .png .jpg .jpeg .webp .csv .json .tex .typ .md
.wav .mp3 .ogg .ttf .otf
```

It excludes generated or volatile trees:

```text
.git .manim-director media output dist target __pycache__ .venv venv
```

Changing source, configuration, an input dataset, image, audio file, or font invalidates the result. Writing another output or build artifact does not. A cache hit is still recorded as a new succeeded job with `cached: true`, which keeps history accurate while returning immediately.

Pass `--set disable_caching=true` when a renderer, external executable, or undeclared input changed without a corresponding project-file change. Use `--set flush_cache=true` only to diagnose invalidation behavior; routine builds should reuse cached intermediates.

## Persistence path

SQLite stores jobs, logs, and result metadata at `.manim-director/state.db`. It uses WAL journaling, `synchronous=NORMAL`, indexed job/log queries, and a five-second busy timeout. Per-job log quotas emit one truncation marker; global retention keeps at most 50,000 events or 64 MiB of encoded log rows by pruning the oldest terminal-job logs. Active-job logs are protected until the job reaches a terminal state. This supports simultaneous SSE reads and job updates without a separate service.

Only structured metadata belongs in SQLite. Rendered videos, stills, contact sheets, ZIP exports, and source files remain ordinary files referenced from result JSON. This avoids database copies of large media and keeps backups/project cleanup unsurprising.

## API and UI budgets

The following are release budgets for a localhost production build. They isolate control-plane regressions from unavoidable render cost:

| Path | Budget |
|---|---:|
| `manim-director --help` cold start | 150 ms p95 |
| `GET /api/health` | 25 ms p95 |
| Cached job submission | 50 ms p95, excluding first project fingerprint |
| Engine event to connected SSE client | 100 ms p95 |
| Workbench interactive after local static load | 1 s on a current laptop |
| Compiled workbench JS + CSS, gzip | 450 KiB total |
| Rust server idle RSS, no Python child | 64 MiB |

Measure these budgets on release builds; they do not cover Manim, TeX, FFmpeg, GPU drivers, or user scene code. A render report should separate queue time, bridge startup, render/media time, QA time, and export time rather than collapsing everything into “the server was slow.”

The Cargo release profile uses thin LTO, one code-generation unit, symbol stripping, and abort-on-panic. Those choices trade a slower release build for a smaller, faster production binary. Development builds prioritize iteration speed.

## Keeping agent context small

Media size and language-model context are separate resource problems. The agent-facing contract follows these rules:

1. Return paths and metadata for media; never base64-encode frames, video, audio, or archives into JSON.
2. Return inventory summaries and requested source spans; do not return every source file by default.
3. Page jobs and logs with opaque cursors. Consumers must continue from `next_cursor` rather than replay earlier pages.
4. Default to the implicated scene, section, time range, or object. Global scans require an explicit request.
5. Represent visual review as a contact-sheet path plus timestamped findings. Open individual images through an image viewer only when visual judgment is required.
6. Represent edits as a semantic request or patch. Do not resend unchanged files.
7. Truncate raw stderr at ingestion and retain the structured error code, failing stage, source location, and artifact path.

For MCP clients, one high-level tool call should normally fit below 64 KiB of text JSON. If a result would exceed that budget, return a cursor, source-span handle, artifact path, or compact summary. The underlying REST API can still serve richer workspace state to the visual workbench.

## Rendering throughput

Choose the cheapest output that answers the current question:

| Need | Operation |
|---|---|
| Validate source/environment | `doctor` or `inspect` |
| Check one layout state | `still` |
| Check opening/middle/final states | `contact_sheet` |
| Check motion/timing | `preview` |
| Deliver final media | `render` |
| Package source and outputs | `export` |

Changing one scene should submit that scene or section, not a full project render. A successful targeted preview can feed final QA without rerunning unrelated scenes. Production resolution should be used only after composition and timing are settled.

OpenGL can improve suitable scenes but brings driver and headless-display costs. Cairo remains the deterministic default. Renderer choice is an output/profile decision rather than a global requirement.

## Backpressure, cancellation, and timeouts

The bounded MPSC queue rejects overflow with `queue_full`; it never grows until the process is out of memory. Worker permits cap simultaneous Python children. Each active job owns a cancellation token. A user cancellation kills the bridge child and records `cancelled`; timeout records `job_timeout` with the configured seconds.

SSE clients consume a broadcast channel. A client that falls behind the channel capacity must refresh compact state and resume from persisted log/job cursors rather than forcing the engine to retain an unbounded event history.

## Profiling a slow job

Work from the narrowest layer:

1. Check whether `cached` is false because source or parameters actually changed.
2. Compare `created_at`, `started_at`, and `finished_at` to separate queue time from execution.
3. Read the newest bounded log page for the job.
4. Run `inspect` to confirm the selected scene and renderer.
5. Preview the changed section at a lower profile.
6. Profile scene construction only when the logs identify Python/Manim as the dominant stage.

Do not raise worker count to compensate for one pathologically slow scene; that usually multiplies contention. Fix high mobject counts, per-frame allocations, expensive updaters, oversized source assets, or repeated TeX compilation in the scene itself.
