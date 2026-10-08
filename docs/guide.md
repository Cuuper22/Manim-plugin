# Guide

This guide covers a project from `init` to delivery. Exact flags, schemas and wire formats are in
the [reference](reference.md).

## Install

Requirements: Python 3.11+, FFmpeg with `ffprobe`, and a TeX distribution with `dvisvgm` for `Tex`
and `MathTex`. On Linux and macOS, pip builds Manim's Cairo (and on Linux its Pango) bindings, which
needs a C compiler, `pkg-config` and the Cairo and Pango development files; the
[README](../README.md#install) lists the packages.

```bash
python3 scripts/install.py --with-manim                 # release binary, Manim and the runtime
python3 scripts/install.py --with-manim --from-source   # build this checkout (Rust and Node 22)
python3 scripts/install.py --with-manim --prefix /opt/manim-director
```

The installer downloads the release archive for your platform and checks it against the release's
`SHA256SUMS` before unpacking; when no release fits, it builds from source if Rust and Node are
present. The binary goes to `<prefix>/bin` (default `~/.local/bin`), and the runtime gets its own
virtual environment in `<prefix>/share/manim-director/venv`, which the engine finds next to itself.
`MANIM_DIRECTOR_PYTHON` points the engine at another interpreter instead; that interpreter needs the
`manim_director_runtime` package and Manim. To uninstall, delete the binary and that `venv`
directory.

**Claude Code** starts the MCP server through `scripts/mcp_launcher.py`, which looks for the engine
on `PATH`, in `$MANIM_DIRECTOR_PREFIX/bin`, in `~/.local/bin` and in the plugin's data directory. It
passes the session's project directory as `--project`. Without an engine it serves one `setup` tool
that prints the install command.

**Codex** runs `manim-director mcp` from the session's working directory, so the engine must be on
`PATH` before the session starts. Open a new thread after installing the plugin.

## Projects

```bash
manim-director init my-film --template derivation --theme paper --name "Completing the square"
```

creates `director.yaml`, `scenes/main.py`, `manim.cfg`, `README.md` and a `.gitignore`. Templates:
`explainer` (default), `derivation`, `geometry`, `graph` and `vertical_short`, plus the five
[gallery](../examples/gallery) films, `picture_to_formula`, `zoom_detail`, `misconception`,
`contrast` and `concrete_first`, which also carry a `brief.viewer`; each is a finished scene whose
beats match the storyboard in `director.yaml`. Inside an existing project,
`manim-director init --scene-template graph` adds `scenes/graph.py`. `init` refuses a non-empty
directory; `--force` writes the template anyway, replacing files of the same names without a copy.

`director.yaml` names the project, the scene files and the main scene, the theme, the storyboard,
render profiles, the safe area and symbol colors. Only `project.name` is required; the
[reference](reference.md#directoryaml) lists every key.

The engine keeps its state in `.manim-director/`: `state.db` (jobs and logs), `artifacts/<job id>/`
(every output), `media/` (Manim's own cache) and `undo/` (the previous content of files changed
through `edit` or the workbench). The `.gitignore` from `init` excludes it.

**Profiles.** `draft` (854×480, 15 fps), `preview` (1280×720, 30 fps, the default), `production`
(1920×1080, 60 fps), `ultra` (3840×2160, 60 fps) and `custom` (`render.width/height/fps`). Projects
can override them or add their own:

```yaml
profiles:
  loop-gif: {resolution: [640, 360], fps: 15, format: gif}
  transparent: {quality: high, format: mov, alpha: true}
```

## Authoring with DirectedScene

```python
from manim import *
from manim_director_runtime import DirectedScene, Region


class Tangent(DirectedScene):
    theme = "paper"                                    # optional; else director.yaml, else midnight
    symbols = {"h": "accent", r"\theta": "#D1495B"}    # merged over director.yaml direction.symbols
```

`DirectedMovingCameraScene` and `DirectedThreeDScene` work the same way; the `Directed` mixin
combines with any other Manim scene base. During a render `self.theme` is the resolved theme, so
`self.theme.primary`, `self.theme.muted` and the other tokens (`background`, `foreground`,
`secondary`, `accent`, `success`) are `#RRGGBB` strings for your own mobjects. Plain Manim objects
default to the theme's foreground color and font.

### Text and math

| Call | Returns |
|---|---|
| `self.title(text)` | A `Text` in the header lane; a previous title cross-fades into it. |
| `self.caption(text)` | At most two lines in the caption lane; `caption(None)` clears it. |
| `self.text(text, role="body")` | `Text` styled by role: `title`, `heading`, `body`, `caption`, `label`. |
| `self.tex(*strings)` | Text-mode `Tex`; symbols inside `$...$` get their colors. |
| `self.math(*strings)` | `MathTex` with symbol colors. A single string is split into atoms so `TransformMatchingTex` can match terms; strings with `&`, `\\` or `{{ }}` stay whole. |

Symbol colors apply to whole TeX tokens: `"r"` colors every `r` in `r^n`, `rS` and
`\frac{1}{1 - r}`, but not the letter inside a command such as `\rho`. Values are theme token
names, `#RRGGBB` or Manim colors such as `YELLOW`.

### Placing things

`self.place(*mobjects, region="content", anchor=None, direction=DOWN, buff=0.4, replaces=None,
min_scale=0.5)` arranges the mobjects along `direction`, shrinks them to fit the region if needed,
and stages them: they enter at the next animation. Needing less than `min_scale` is a
`CompositionError`. Regions: `safe` (the whole safe area), `header`, `content`, `left`, `right`,
`top`, `bottom`, `caption`. `anchor=(x, y)`, each in -1..1, moves the group from the region's center
toward its edges: `(0, -1)` sits on the bottom edge.

- Objects already on stage glide to their new place, so `place(VGroup(old, new))` can gather
  existing and new objects.
- `replaces=old` morphs an on-stage object into the new one.
- A placement that would overlap something still on stage raises `CompositionError` before anything
  moves; the message says how to fix it (place both in one call, use another region, or let the next
  beat retire the old object).

### Beats

```python
with self.beat("limit", transition="continue", keep=[graph], hold=1.0):
    ...
```

Nothing moves when a beat is entered. At its first animation (or at its end), everything on stage
that was not kept or placed again leaves, carried objects glide or morph, and staged objects enter,
in one transition:

| `transition` | What the viewer sees |
|---|---|
| `continue` (default) | Kept and replaced objects glide or morph; the rest fades out. |
| `contrast` | The old slides out as the new slides in. |
| `reveal` | New objects are drawn. |
| `chapter` | The stage clears, title and caption included. |

`focus=` dims everything else once that object is on stage, and on exit the beat holds for `hold`
seconds (default 1). `run_time` sets the transition's length. `intent`, `question` and `takeaway`
are notes for readers of the code; the storyboard in `director.yaml` holds the same plan
(`intent`, `audience_question`, `takeaway`) for the workbench. Beats do not nest. Each beat starts a
Manim section named after it (unnamed beats are `beat-1`, `beat-2`, ...) and is recorded with its
file and line in the render timeline.

### Derivations

```python
steps = self.derive(
    r"(x + h)^2 - x^2",
    (r"= 2xh + h^2", "expand"),
    (r"= h(2x + h)", "factor"),
    region=Region.RIGHT,
)
```

Each step is a TeX string or a `MathTex`, optionally paired with a note (`$...$` in a note is TeX).
The first step is written (or morphed from `replaces=`), then each line transforms into the next
with `TransformMatchingTex`, stacked with relations in one column. Notes go beside the lines or
under each line, whichever needs less shrinking (`notes="right"` or `"below"` to choose).
`in_place=True` transforms a single line instead. `run_time` and `pause` set each step's length
and the rest between steps. The result is a `Derivation` (a `VGroup`) with `.lines` and `.notes`;
`steps.lines[-1]` is what the next beat usually replaces.

- `self.term(eq, r"\frac{b}{2a}", occurrence=None)` returns the glyphs of a sub-term of any `MathTex`
  or `Tex`.
- `self.highlight(eq, r"b^2 - 4ac", color="accent", box=False)` recolors sub-terms (or the whole
  expression) and, with `box=True`, backs each occurrence with a soft box that moves and leaves
  with `eq` (the result's `.boxes`); `color=None` keeps symbol colors and only boxes.
- `self.tag(eq, label=None)` puts an equation number at the right edge of the region `eq` is placed
  in, level with `eq`, and moves it along with `eq`; labels count `(1)`, `(2)`, ... unless given.
- `self.focus(*mobjects)` dims everything else on stage (title and caption stay lit);
  `self.unfocus()` restores it.

### Frame shape

Manim takes the frame's shape from `manim.cfg`, not from `-q` or `-r`. A `DirectedScene` whose frame
and pixel aspect ratios differ raises `CompositionError` instead of rendering squashed. Portrait
projects set `pixel_width`, `pixel_height` and `frame_rate` in `manim.cfg`, as the `vertical_short`
template does, plus portrait profiles and a wider `safe_area`.

## The verify loop

Render small, look, fix, repeat. Every command below runs as a job; add `--json` for the full job
record.

```bash
manim-director render --scene Tangent --profile draft
manim-director render --scene Tangent --sections         # also one video per beat
manim-director still --scene Tangent                     # the last frame, no video
manim-director frame --scene Tangent --at 4.2
manim-director contact-sheet --scene Tangent --count 9 --columns 3
manim-director qa --scene Tangent --frames 12
```

`--scene` takes a class name or a scene id from `director.yaml`; `--file` names the file when two
files define the same class. `frame`, `contact-sheet`, `qa` and media exports read the latest
successful render of the scene (at `--profile`, when given), or an explicit `--job <id>` or
`--path <file>`.
`qa` also checks a PNG from `still` or `frame`.

**What `qa` checks.** It samples frames and reports `blank_frame` (error), `low_contrast` (warning,
below 3:1 against the background) and `safe_area` (warning, content outside the margins from
`safe_area`). Each finding carries the time, the beat and the line where that beat starts. It does
not judge whether the animation is clear or correct; look at the frames.

**Checking the algebra.** `validate-math` checks that consecutive steps are equal, symbolically
with SymPy when it can and numerically at sampled points:

```bash
manim-director validate-math "(x + h)^2 - x^2" "2*x*h + h^2" "h*(2*x + h)"
manim-director validate-math "sqrt(x^2)" "x" --range x=0:5
```

Expressions use Python syntax with `^` for powers; an equation is checked as `lhs - rhs`, divided by
any factor the step applied to both sides. Variables are sampled in -10..10 unless
`--range` says otherwise, and a range starting at or above 0 tells SymPy the variable is
nonnegative: without `--range x=0:5` the second check fails at a negative `x`. A failed check is a
successful job whose verdict is "a step is not equivalent"; `--json` gives the counterexample.

**When a render fails** the job's error already holds `file:line` findings classified as Python,
LaTeX, FFmpeg, font, OpenGL, missing-asset or composition errors. `diagnose --job <id>` prints them
again, and `diagnose --text` (or `--text-file`) reads a pasted traceback or TeX log.

**Caching.** A render or still whose scene, settings, runtime and project files (the
[reference](reference.md#cli) lists which) have not changed returns the earlier job's artifacts at
once (`cached`), and submitting one identical to a render still running returns that job. Renders
and stills of one scene run one at a time; the later one shows as running ("Waiting for another
render") without holding a worker. `--fresh` renders again and also skips Manim's partial-movie
cache.

## Delivering

```bash
manim-director render --scene Tangent --profile production
manim-director export --format mp4 --scene Tangent                 # output/Tangent.mp4
manim-director export --format gif --scene Tangent --gif-fps 15 --gif-width 640
manim-director export                                              # output/<project>.zip
```

A zip holds `director.yaml`, `manim.cfg`, `pyproject.toml`, READMEs, licenses and requirements
files at the root, the source, asset and `sources/` directories, and the files `director.yaml`
refers to. It leaves out `.manim-director/`, the output and media directories, the ignored
directories (`.git`, `.venv`, `node_modules`, ...) and files that look like credentials, and lists
every entry in `manim-director-export.json`. The latest render of `--scene` (default the main
scene), or `--job`, adds its artifacts under `deliverables/`. `--output` picks another path inside
the project.

`captions` validates a WebVTT or SRT file, shifts or scales its times, and converts between the two:
`manim-director captions captions.vtt --shift 0.5 --output captions.srt`.

`ingest` copies files from outside the project: documents and data go to `sources/`, images, SVG,
audio and video to the asset directory. `sources/manifest.json` records each file's origin, SHA-256,
license and attribution, and the job result summarizes it (headings, CSV columns and rows, pages,
image size, duration). `--normalize` strips scripts from SVG, scales images larger than 4096 px down
and evens out audio loudness.

```bash
manim-director ingest ~/notes/recurrences.md ~/data/sequences.csv --license CC-BY-4.0
```

## The workbench

`manim-director open` starts the server on `127.0.0.1:4177` and opens the workbench in your browser
already signed in; `serve` does the same without the browser. Both print the sign-in link, which
carries a token that changes every start. Use `--port` for another port and `--port 0` for any free
one. A port already in use is most likely an engine that is already serving: open the link it
printed.

The workbench shows the project's scenes with their storyboard beats, the latest render of the
selected scene over a beat timeline (tagged outdated once its scene changes, with the `.srt` from
Manim's `add_subcaption` as a download), stills and contact sheets, the source editor, and the QA
and doctor findings; a finding opens its file at its line. Saves are checked against the file's
revision, so an edit made elsewhere (by you or an agent) is never overwritten silently. Jobs from
the CLI and MCP appear as they run.

`--allow-remote` lets other machines connect over plain HTTP; anyone with the link can then edit
files and run code as you. Prefer an SSH tunnel.

## Working with an agent

The skill teaches the agent this guide's loop: inspect the project, plan beats, write the scene with
`DirectedScene`, render a draft, look at a contact sheet or stills, run `qa`, fix, and render the
final profile. The [MCP tools](reference.md#mcp-tools) take the CLI's parameters; `submit` runs
`frame`, `diagnose`, `captions`, `ingest` and `export`, and `job_status` follows or cancels a job
that outlived the tool's wait. Answers give the verdict and artifact paths instead of media, so the
agent opens only the images it needs.

Agents edit files with their own tools; their jobs show up in a running workbench too.

## Troubleshooting

Run `manim-director doctor` first. It checks the interpreter, Manim, NumPy, Pillow, PyAV, PyYAML,
FFmpeg, TeX with `dvisvgm`, SymPy, pypdf, OpenGL and free disk space, and says whether the project
is ready to render.

- **`runtime_unavailable`**: the engine could not start Python or the runtime. Rerun the installer,
  or set `MANIM_DIRECTOR_PYTHON` to an interpreter with `manim_director_runtime` installed.
- **A missing Python package**: install it into the runtime's environment, for example
  `~/.local/share/manim-director/venv/bin/python -m pip install sympy`, or rerun
  `install.py --with-manim`.
- **`latex_missing`**: install TeX with `dvisvgm`; `Text` still works without it, `MathTex` does not.
- **OpenGL unavailable**: use the default `cairo` renderer; headless machines usually lack a display.
- **A job left running by a crashed engine**: once that engine's heartbeat has been silent for 10
  seconds, any running engine (a CLI command, `serve` or `mcp`) fails its jobs as `engine_lost`
  within two seconds and releases their scene locks.
