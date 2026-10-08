# Tools, jobs and errors

## Arguments

| Tool | Arguments (all optional unless marked) |
|---|---|
| `inspect` | none; answers `scenes` (`scene_id`, `declared_id`, `beats`), `latest` per scene (`video`, `video_profile`, `video_outdated`, `still`, `contact_sheet`), `findings`, `recent_jobs` |
| `init` | `template` (`explainer`), `name`, `theme`, `seed`, `force`; or only `scene_template` to add a scene to the existing project |
| `doctor` | `wait_seconds` |
| `render` | `scene` (class or `director.yaml` scene id; default `engine.main_scene`), `file` (when two files define the class), `profile` (`draft`, `preview` default, `production`, `ultra`, `custom`, or a project profile), `sections`, `fresh` |
| `still` | `scene`, `file`, `profile`, `fresh` |
| `contact_sheet` | `source`, `scene`, `profile`, `count` (6, ≤ 24), `columns` (3, ≤ 8) |
| `qa` | `source`, `scene`, `profile`, `frames` (8, ≤ 40) |
| `validate_math` | `steps` (required, 2–32), `ranges` (`{"x": [0, 5]}`), `samples` (200), `tolerance` (1e-9), `seed` |
| `submit` | `operation` (required) plus that operation's arguments, below |
| `job_status` | `job_id` (required), `cancel`, `cursor`, `limit` (20, ≤ 100) |

Every job tool also takes `wait_seconds` (20, 0–50).

`submit` operations:

- `frame`: `at_seconds` (required), `source`, `scene`, `profile`.
- `diagnose`: `job_id` of a failed job, or `text` (a traceback or TeX log).
- `export`: `format` (`zip` default, `mp4`, `webm`, `gif`), `source`, `scene`, `profile`,
  `output` (project path; default `output/<scene>.<format>` or `output/<project>.zip`), `gif_fps`,
  `gif_width`.
- `captions`: `path` (`.vtt`/`.srt`, required), `shift_seconds`, `scale`, `output` (converts by
  extension).
- `ingest`: `sources` (required: `[{"path": "/absolute/host/path", "id", "license",
  "attribution"}]`), `normalize`, `force`. Copies files into `sources/` or the asset directory with
  provenance in `sources/manifest.json`. Credential files and stores (`.env`, `.ssh/`, `.aws/`,
  `.kube/`, ...) and `/proc`, `/sys`, `/dev` are refused.
- Also `doctor`, `render`, `still`, `contact_sheet`, `qa`, `validate_math` with the arguments above.

**Sources.** `frame`, `contact_sheet`, `qa` and media `export` read `source: {"job_id": "…"}` (a
succeeded render, export, or for `qa` also a `still`/`frame` image) or `source: {"path": "…"}` (a
project file). Without `source` they use the latest successful render of `scene` (at `profile`,
when given); with nothing rendered yet they fail with `source_not_found`: render first.

## Answers and jobs

The text content is `<operation> <job id> <status>`, the verdict (`qa: warn`, `ready to render:
no`, `a step is not equivalent` with the failing step pair and a counterexample), up to five
findings as `severity file:line: message` with hints, and every artifact's absolute path, one per
line. `structuredContent` is `{job, result, error, paths}`, where `paths` holds those absolute paths;
open PNGs from there.

- `job`: `id`, `operation`, `status` (`queued`, `running`, `succeeded`, `failed`, `cancelled`),
  `cached`, `scene_id`, `progress`, timestamps.
- `result` (when succeeded): per operation, e.g. `render` gives `scene`, `duration_seconds`,
  `animations` and `artifacts` (video, timeline, per-beat sections, `.srt` from `add_subcaption`;
  each with `media` details); `qa` gives `status` (`pass`/`warn`/`fail`), `frames` with metrics,
  `findings` (pixel checks, plus pacing and viewer checks for a `DirectedScene`) and the
  `beats.png` contact sheet; `validate_math` gives `valid`, and per step pair `equivalent`, the
  symbolic difference and a numeric `counterexample`.
- `error` (when failed): `{code, message, data}`; `render_failed` puts `findings`, `exception` and
  the `traceback` tail in `data`.

A job still running after `wait_seconds` comes back with its status: call `job_status` with its
`job_id` (and `wait_seconds` up to 50) rather than submitting again. `job_status` also returns log
`events` after `cursor`; pass the returned `next_cursor` next time. `cancel: true` stops it.

Renders and stills are cached on the scene, settings, runtime and the project's code, config, data,
TeX, shader, image, audio and font files: an unchanged request returns at once with `cached: true`.
`fresh: true` bypasses that and Manim's own cache; use it after changing any other file a scene
reads. Renders and stills of one scene run one at a time: the later one reports `running`
("Waiting for another render of …") until the first ends. Other jobs run two at once by default.

## Cheapest check first

| Question | Call |
|---|---|
| Does the environment work? | `doctor` |
| What is in the project? | `inspect` |
| Does the final layout fit? | `still` |
| How does the whole scene flow? | `render` at `draft`, then `contact_sheet` |
| What happens at 7.5 s? | `submit` `frame` with `at_seconds` |
| Blank, low-contrast or clipped frames; pacing for the viewer? | `qa` |
| Does each beat answer its question? | `qa`, then open `beats.png` |
| Is the algebra right? | `validate_math` |
| Final file | `render` at `production`, then `submit` `export` |

## Errors and what to do

| `error.code` | Meaning | Do |
|---|---|---|
| `invalid_params` | A bad argument; `data.field`, `reason`, often `allowed` | Fix the argument; `allowed` lists the choices (profiles, templates, themes). |
| `invalid_spec` | `director.yaml` is missing or invalid; `data.line` | Fix that line; `init` if there is no project. |
| `source_not_found` | Nothing rendered yet for that scene/profile | `render` first. |
| `scene_not_found`, `scene_ambiguous`, `scene_required` | Class name wrong, defined twice, or not given | Use a name from `inspect`; pass `file`. |
| `render_failed` | The scene raised; `data.findings` with `file:line` | Fix the first finding; see codes below. |
| `dependency_missing`, `runtime_unavailable` | Python, Manim, FFmpeg or TeX missing | `doctor`; tell the user what to install. |
| `timeout` | Over the job timeout (30 min default) | Shorten the scene or raise `budgets.render_seconds`. |
| `queue_full`, `engine_lost` | Too many jobs; an engine process died | Wait and resubmit. |
| `invalid_expression` | `validate_math` could not parse a step; `data.step`, `column` | Python syntax, explicit `*`, one expression per step. |
| `artifact_invalid`, `media_error`, `internal` | The engine or FFmpeg rejected the output | Retry once with `fresh: true`, then report it with the job id. |

Finding codes in a `render_failed` or `diagnose` result: `composition` (a `DirectedScene` rule;
follow the message), `python_syntax`, `python_import`, `python_name`, `python_attribute`,
`api_signature` (wrong Manim API for 0.21), `latex_missing`, `latex_package`, `latex_error` (TeX
near the shown snippet), `ffmpeg_encoder`, `ffmpeg_mux`, `font_missing`, `opengl_context` (use
Cairo), `asset_missing`, `unclassified`.

## The CLI

Use it for another project (`--project <dir>`), in a shell-only session, or for things MCP does not
do (`edit`, `serve`, `open`). It mirrors the tools: `manim-director render --scene S --profile
draft`, `contact-sheet`, `qa`, `frame --at 7.5`, `validate-math "a^2 - b^2" "(a - b)*(a + b)"`
(steps that start with `-` go after `--`), `diagnose --job ID`, `export --format gif`, `inspect`.
`--json` prints the full job record. The CLI, MCP and the workbench share the project's database,
so jobs from any of them show up in all three.
