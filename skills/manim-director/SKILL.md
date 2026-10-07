---
name: manim-director
description: Author, render, check and debug Manim Community animations, especially math-heavy ones (derivations, proofs, graphs, geometry, data), with the local Manim Director engine and its DirectedScene API. Use for any request to create or change a Manim scene, animate mathematics, render or preview a scene, look at frames or a contact sheet, fix a failing or badly laid out render, check the algebra in an animation, or export video and GIFs.
metadata:
  short-description: Author and verify math-heavy Manim animations
---

# Manim Director

You write ordinary Manim Community scenes on top of `DirectedScene` (a math-first layer: beats,
layout regions, derivations, theme-aware symbol colors) and use the local engine's MCP tools to
render them, look at the frames, check them and fix them. Edit files with your own file tools; the
engine renders, inspects and checks.

## Start from the project's state

1. Call `inspect`. It lists the scenes the engine found (with beat counts), profiles, the theme,
   the latest render, still and contact sheet per scene, and recent jobs.
   - `director.yaml is missing`: call `init` with a `template` (`explainer`, `derivation`,
     `geometry`, `graph`, `vertical_short`), or ask where the project should live. To add a scene to
     an existing project, call `init` with only `scene_template`.
   - No MCP tools at all, or only a `setup` tool: the engine is not installed. In Claude Code,
     `setup` prints the exact command. Otherwise give the user
     `python3 <this skill's directory>/../../scripts/install.py --with-manim` (Python 3.11+; it
     also needs FFmpeg and TeX with dvisvgm). Codex starts `manim-director mcp` from `PATH`, so the
     installer's `bin` directory (`~/.local/bin` by default) must be on `PATH`; then ask them to
     restart the session.
2. Call `doctor` before the first render on an unfamiliar machine, and after any
   `runtime_unavailable` or `dependency_missing` error. It says whether the project is ready to
   render and what is missing.

The MCP server serves one project: the one at or above the directory the session started in. For
another project, use the CLI with `--project`.

## Plan the beats before writing code

For each beat decide: its id, the question in the viewer's mind, the one takeaway, the object the
viewer tracks, what stays on stage from the previous beat (`keep=`), and the transition
(`continue`, `contrast`, `reveal`, `chapter`). One idea per beat; if a takeaway needs "and", split
it. Pick one visual spine (an object, picture or expression the viewer keeps recognizing) and give
each color one meaning for the whole film. Record the beats in `director.yaml` under `storyboard`
with ids equal to the beat ids in the scene. Infer ordinary aesthetic choices; ask one focused
question only when two readings of the request would produce different mathematics.

## Write the scene

```python
from manim import *
from manim_director_runtime import DirectedScene


class CompletingTheSquare(DirectedScene):
    symbols = {"a": "primary", "b": "secondary", "c": "accent"}

    def construct(self):
        claim = self.math(r"ax^2 + bx + c = 0")
        with self.beat("claim", transition="reveal"):
            self.title("Where the quadratic formula comes from")
            self.place(claim)
            self.caption("Any quadratic with a ≠ 0.")

        with self.beat("complete"):
            self.caption("Add exactly what makes the left side a square.")
            steps = self.derive(
                r"ax^2 + bx + c = 0",
                (r"x^2 + \frac{b}{a}x = -\frac{c}{a}", "divide by a"),
                (r"\left(x + \frac{b}{2a}\right)^2 = \frac{b^2 - 4ac}{4a^2}", "complete the square"),
                replaces=claim,
            )
        self.highlight(steps.lines[-1], r"b^2 - 4ac", color="success", box=True)
        self.wait()
```

Rules that keep scenes correct and readable:

- Formulas go through `self.math` (symbol colors, atoms that `TransformMatchingTex` can match);
  algebra goes through `self.derive`, one justified step per line, with a short note when the step
  is not obvious. Put symbol colors in `symbols` or `director.yaml` `direction.symbols`, never
  per formula.
- Position top-level content with `self.place(..., region=...)` and the `title`/`caption` lanes,
  not `move_to`/`shift` chains. Use `left`/`right` or `top`/`bottom` for a picture beside its
  algebra. Morph with `replaces=` rather than fading one expression out and another in.
- Use plain Manim for diagrams, graphs and updaters, colored with `self.theme.primary` and the other
  tokens. Objects you `self.play` in are on stage and leave at the next beat unless kept.
- A `CompositionError` is the layout refusing an overlap or an unreadable scale. Do what its message
  says (place together, use another region, let the next beat retire the object, shorten); never
  work around it with manual offsets.
- Keep mathematics, data and the user's claims exactly as given. Distinguish proof, numerical
  evidence and intuition in captions.

The full API, transition semantics, regions and pitfalls are in
[references/authoring.md](references/authoring.md). `director.yaml` keys are in
[references/project.md](references/project.md).

## Verify every change by looking at it

1. `render` the changed scene at `profile: "draft"`. Render only what changed.
2. `contact_sheet` for the whole scene; `still` (last frame) or `submit` with `operation: "frame"`
   and `at_seconds` for one moment. Open the PNG paths from the answer with your image viewer and
   look: overlaps, clipped or tiny formulas, wrong colors, empty frames, a confusing order.
3. `qa`. Its findings name the time, the beat and the line where that beat starts.
4. `validate_math` on the algebra behind each derivation: Python syntax, `^` allowed, each equation
   written as `lhs - rhs` and divided by any factor the step applied to both sides, `ranges` for
   domain assumptions.
5. Fix and repeat. Stop after two or three passes that do not converge and report what remains.
6. Render the requested profile (`production` by default for delivery) and `export` if a file is
   wanted.

A successful render is not a correct animation. Never call work done without having looked at the
opening, the main transformation and the final frame of each changed scene.
[references/verify.md](references/verify.md) covers what each check does and does not catch, and
the acceptance checklist.

## Tools

| Tool | Use |
|---|---|
| `inspect` | Project summary; call first. |
| `init` | New project from a template, or `scene_template` to add a scene. |
| `doctor` | Environment check. |
| `render` | `scene`, `profile`, `sections`, `fresh`. |
| `still` | Last frame as PNG; cheapest layout check. |
| `contact_sheet` | Evenly spaced frames of the latest render (`count`, `columns`). |
| `qa` | Blank frames, low contrast, safe-area violations. |
| `validate_math` | Are consecutive steps equal (SymPy plus sampling)? |
| `submit` | `frame`, `diagnose`, `captions`, `ingest`, `export` (`operation` plus its parameters). |
| `job_status` | Follow a job past the tool's wait, page its log, or `cancel` it. |

Job tools wait up to `wait_seconds` (20 by default, at most 50). A job still running comes back with
its id: call `job_status` with that `job_id` instead of submitting again. An identical render
returns the cached result at once; `fresh: true` forces a new one. Parameters, sources and error
codes are in [references/tools.md](references/tools.md).

## When a render fails

The failed job's `error` holds `findings` with a code, a message, a hint and `file:line`. Fix that
line first and rerender; downstream errors usually disappear with the first one. For a traceback or
TeX log the user pastes, `submit` `{"operation": "diagnose", "text": "…"}`. When the cause is the
environment (`runtime_unavailable`, `dependency_missing`, or a `latex_missing` or `font_missing`
finding), run `doctor` and tell the user what to install rather than changing the scene.

## Be honest about limits

- `qa` measures pixels: it cannot see overlapping formulas inside the content area, unreadable
  notation or wrong mathematics. Your own look at the frames is the real check.
- `validate_math` checks expression equality (an equation only as `lhs - rhs`), not LaTeX,
  inequalities or limits.
- There is no voice-over or speech synthesis; captions are validated and retimed, not written.
- Manim Community 0.21 only (not ManimGL). OpenGL rendering needs a display; Cairo is the default.

## Handoff

Lead with what now works: the scenes changed, the output paths, the frames you inspected and what
the checks said. Then anything unresolved, with the scene, time and evidence. Skip routine tool
narration and long logs.
