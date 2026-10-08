---
name: manim-director
description: Author, render, check and debug Manim Community animations, especially math-heavy ones (derivations, proofs, graphs, geometry, data), with the local Manim Director engine and its DirectedScene API. Use for any request to create or change a Manim scene, animate mathematics, make a math explainer, render or preview a scene, look at frames or a contact sheet, fix a failing or badly laid out render, check the algebra in an animation, or export video and GIFs.
metadata:
  short-description: Author and verify math-heavy Manim animations
---

# Manim Director

You write ordinary Manim Community scenes on top of `DirectedScene` (beats, layout regions,
derivations, theme-aware symbol colors and a kit of explainer components) and use the local
engine's MCP tools to render them, look at the frames, check them and fix them. Edit files
yourself; the engine renders, inspects and checks.

## Start from the project's state

1. Call `inspect`: scenes (with beat counts), profiles, theme, the latest render, still and
   contact sheet per scene, and recent jobs.
   - `director.yaml is missing`: call `init` with a `template`, or copy a gallery film (*Starting
     points* in [references/authoring.md](references/authoring.md)); or ask where the project
     should live. To add a scene to an existing project, call `init` with only `scene_template`.
   - No MCP tools, or only `setup`: the engine is not installed. In Claude Code, `setup` prints
     the command; otherwise give the user `python3 <this skill's directory>/../../scripts/install.py
     --with-manim` (Python 3.11+, FFmpeg, TeX with dvisvgm; its `bin` directory must be on
     `PATH`), then ask them to restart the session.
2. Call `doctor` before the first render on an unfamiliar machine, and after any
   `runtime_unavailable` or `dependency_missing` error.

The MCP server serves the project at or above the session's directory; for another project, use
the CLI with `--project`.

## Model the viewer, then plan the beats

You know the answer; the viewer does not. They see only frames and captions (there is no
voice-over), cannot interrupt, and look at whatever moves or is brightest. Plan every beat from
what they believe at that moment. Before any code, write `brief.viewer` in `director.yaml`:

```yaml
brief:
  viewer:
    who: curious 16-year-old, knows algebra, watching on a phone
    level: general        # intro | general | expert: sets qa pacing and default holds
    knows: [odd numbers, square numbers, $n^2$]
    new: [the L shape, $1 + 3 + \cdots + (2n-1)$]          # each shown before it is named
    question: Why do 1, 1+3, 1+3+5 keep landing on squares?  # their words, no jargon
    wrong_guess: a coincidence that breaks for big numbers    # what they predict, and why
    aha: each odd number is an L that grows k×k into (k+1)×(k+1)
    payoff: can say 1+3+…+19 = 100 at a glance, and why
```

Then one `storyboard` entry per beat, ids in the scene's order:

```yaml
storyboard:
  - id: grow
    intent: explain        # introduce | explain | compare | reveal | prove | recap
    audience_question: If I add the next odd number, what shape do I get?
    changes: the L of 7 wraps the 3×3 square     # one verb; two verbs make two beats
    takeaway: Adding 7 turns 3×3 into 4×4.
    keep: [dots, total]    # what must stay visible: pass it to keep=
    aha: true              # exactly one beat; mirror it with beat(..., aha=True)
```

- Open on the question as the title, with a concrete, surprising instance; never a definition.
- Concrete before symbolic: numbers or objects, then a labeled picture, then the formula, with
  the concrete version still on screen.
- Before the aha, `self.ask(...)` for a prediction; let the `wrong_guess` fail on screen, then
  repair it.
- Each beat answers the previous beat's question with one change, at most 3 new things and one
  hero. Label things on the picture, in their formula colors.
- Captions: at most 12 words, saying why or what to notice, never reading a formula aloud.
  Titles: at most 8 words.
- End by replaying the aha quickly, with the question and its answer both on screen.

Start from the gallery template whose pattern fits (`init --template <name>`).
Infer ordinary aesthetic choices; ask one focused question only when two readings of the request
would produce different mathematics. Budgets, patterns, anti-patterns and the critique
checklist are in [references/viewer.md](references/viewer.md).

## Write the scene

```python
from manim import *
from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import DotArray


class OddSquares(DirectedScene):
    def construct(self):
        dots = DotArray(4, shown=lambda r, c: max(r, c) < 3, radius=0.2, gap=0.3, color="primary")
        total = self.math("1 + 3 + 5 = 9")
        with self.beat("hook", transition="reveal"):
            self.title("Why do odd numbers add up to squares?")
            self.place(dots, region="left")
            self.place(total, region="right")
            self.caption("The first three odd numbers make a 3×3 square.")

        with self.beat("predict", keep=[dots, total]):
            self.ask("Add the next odd number, 7. What shape do you get?")

        with self.beat("grow", keep=[dots, total], aha=True):
            self.caption("Two sides of 3, plus a corner: 7 dots.")
            ring = dots.select(lambda r, c: max(r, c) == 3, color="accent")
            self.show(ring, run_time=2)
            self.annotate(ring, "7", style="label", side="right")
            self.pause()
            self.place(self.math("1 + 3 + 5 + 7 = 16"), region="right", replaces=total)
```

- Reach for the kit before plain Manim: `FunctionPlot`, `Readout`, `DotArray`, `VectorGrid` and
  `Figure` from `manim_director_runtime.kit`, and `self.show`, `annotate`, `link`, `ask` and
  `misconception`. `place` positions top-level content; `show` reveals what belongs to placed
  content (overlays such as `plot.tangent(x)`, reserved parts). See
  [references/kit.md](references/kit.md).
- Write no timing: holds, derive pauses, the prediction pause and the final still follow the
  viewer's `level`. Give only the aha's motion a `run_time` (1.5–3 s). Devices wait until the
  last change has been read; call `self.pause()` before a plain `play` or `place` that follows a
  reveal.
- Formulas go through `self.math`; algebra through `self.derive`, one justified step per line,
  with a short note when the step is not obvious. Put symbol colors in `symbols` or
  `director.yaml` `direction.symbols`, never per formula.
- Position with `self.place(..., region=...)` and the `title`/`caption` lanes, not
  `move_to`/`shift` chains; morph with `replaces=`, never fade one expression out and another in.
  Color plain Manim with theme tokens (`self.theme.primary`).
- A `CompositionError` is the layout refusing an overlap or an unreadable scale. Do what its
  message says; never work around it with manual offsets.
- Keep mathematics, data and the user's claims exactly as given. Distinguish proof, numerical
  evidence and intuition in captions.

The full API is in [references/authoring.md](references/authoring.md); `director.yaml` keys in
[references/project.md](references/project.md).

## Verify every change by looking at it

1. `render` the changed scene at `profile: "draft"`. Render only what changed.
2. `qa`: pixel checks plus pacing and viewer findings from the beat timeline, each with the time,
   beat and source line. Fix every warning; fix or explain every info finding.
3. Open `qa`'s `beats.png`: one settled frame per beat, under its audience question. As the viewer
   in `brief.viewer`, answer each question from the frame alone; where your answer differs from the
   takeaway, that beat fails. Would they know the film's question within 3 s of the first frame?
   Are the question and its answer both on the final frame?
4. `contact_sheet`, `still` or `submit` `frame` at `at_seconds` for other moments: look for
   overlaps, clipped or tiny formulas, wrong colors and a confusing order.
5. `validate_math` on each derivation's algebra: Python syntax, each equation as `lhs - rhs`
   divided by any factor the step applied to both sides, `ranges` for domain assumptions.
6. Fix and repeat. Stop after two or three passes that do not converge and report what remains.
7. Render the requested profile (`production` by default for delivery) and `export` if a file is
   wanted.

A successful render is not a correct or clear animation. Never call work done without having
looked at every beat's frame. [references/verify.md](references/verify.md) lists every check,
what it does not catch, and the acceptance checklist.

## Tools

| Tool | Use |
|---|---|
| `inspect` | Project summary; call first. |
| `init` | New project from a template, or `scene_template` to add a scene. |
| `doctor` | Environment check. |
| `render` | `scene`, `profile`, `sections`, `fresh`. |
| `still` | Last frame as PNG; cheapest layout check. |
| `contact_sheet` | Evenly spaced frames of a render (`count`, `columns`). |
| `qa` | Blank frames, contrast, safe area; pacing and viewer findings; `beats.png`. |
| `validate_math` | Are consecutive steps equal (SymPy plus sampling)? |
| `submit` | `frame`, `diagnose`, `captions`, `ingest`, `export` (`operation` plus its parameters). |
| `job_status` | Follow a job past the tool's wait, page its log, or `cancel` it. |

A job still running after `wait_seconds` (20 by default) comes back with its id: call
`job_status` rather than submitting again. Parameters, caching and error codes are in
[references/tools.md](references/tools.md).

## When a render fails

The failed job's `error` holds `findings` with a code, a message, a hint and `file:line`. Fix the
first one and rerender; later errors often follow from it. For a traceback or
TeX log the user pastes, `submit` `{"operation": "diagnose", "text": "…"}`. When the cause is the
environment (`runtime_unavailable`, `dependency_missing`, or a `latex_missing` or `font_missing`
finding), run `doctor` and tell the user what to install rather than changing the scene.

## Be honest about limits

- `qa` cannot judge whether a frame explains anything, or whether the mathematics is right; your
  frames-only reading of `beats.png` is the real check.
- `validate_math` checks expression equality (an equation only as `lhs - rhs`), not LaTeX,
  inequalities or limits.
- There is no voice-over or speech synthesis; captions are validated and retimed, not written.
- Manim Community 0.21 only (not ManimGL). OpenGL needs a display; Cairo is the default.

## Handoff

Lead with what now works: the scenes changed, the output paths, the frames you inspected and what
the checks said. Then anything unresolved, with scene, time and evidence. Skip tool narration and
long logs.
