# Authoring with DirectedScene

Read this before writing or substantially changing a scene. Everything here is plain Manim
underneath: helpers return ordinary mobjects, and any Manim code mixes in. Plan the film first
with [viewer.md](viewer.md); build it from the components and devices in [kit.md](kit.md).

## Scene classes and theme

```python
from manim import *
from manim_director_runtime import DirectedScene


class Roots(DirectedScene):
    theme = "paper"                       # optional; else director.yaml `theme`, else midnight
    symbols = {r"\lambda": "accent", "p": "primary", "q": "#D1495B"}
```

- `DirectedScene`, `DirectedMovingCameraScene` (title and caption stay put on screen while the
  camera pans or zooms; placed content moves with the scene) and `DirectedThreeDScene` (placed
  objects are fixed in frame as overlays). The `Directed` mixin combines with other scene bases:
  `class Zoom(Directed, ZoomedScene)`.
- Themes: `midnight` (default, dark), `paper` (light, serif), `chalkboard` (dark green), `contrast`
  (black, maximum contrast). Tokens: `background`, `foreground`, `primary`, `secondary`, `accent`,
  `muted`, `success`. During `construct`, `self.theme.primary` etc. are `#RRGGBB` strings. A variant:
  `theme = themes.theme("paper").with_colors(accent="#D1495B")` after
  `from manim_director_runtime import themes`.
- `symbols` maps a TeX token to a color token, `#RRGGBB` or a Manim color (`YELLOW`), merged
  over `direction.symbols` in `director.yaml`. Every occurrence of the token is colored, including
  inside `\frac{..}{..}`; control words are single tokens (`"r"` does not touch `\rho`).
- Plain `VMobject`s (dots and rectangles too) default to the theme's foreground, highlight shapes
  and animations (`SurroundingRectangle`, `Indicate`, `Flash`) to its accent, and `Text` to its
  font for the render.

## Text and math helpers

| Call | Notes |
|---|---|
| `self.title(text)` | Header lane; replaces the previous title with a cross-fade. |
| `self.caption(text)` | Caption lane, wrapped to at most two lines; `caption(None)` clears it. |
| `self.text(text, role=...)` | Roles `title`, `heading`, `body` (default), `caption`, `label` (small, muted). |
| `self.tex(*strings)` | Text-mode LaTeX; `$...$` parts get symbol colors. |
| `self.math(*strings, **kw)` | `MathTex` with symbol colors. One string is split into atoms so `TransformMatchingTex` matches terms between steps; strings containing `&`, `\\`, `\over` or `{{ }}` stay whole. `font_size=72` for a hero formula. |

Prefer one `self.math(...)` per expression over hand-split `MathTex` pieces.

## Layout: regions and place

Regions (inside `safe_area`): `header` and `caption` lanes, `content` between them, `left`/`right`
and `top`/`bottom` halves of `content`, and `safe` (everything, for one hero visual).
`self.region("left")` returns its `Rect` (`left`, `right`, `top`, `bottom`, `width`, `height`,
`center`).

`self.place(*mobjects, region="content", anchor=None, direction=DOWN, buff=0.4, replaces=None,
min_scale=0.5)`:

- Arranges the mobjects along `direction`, shrinks them to fit if needed (never enlarges), and
  stages them; they enter at the next animation with the beat's transition.
- `anchor=(x, y)` in -1..1 moves the group toward the region's edges (`(0, -1)`: bottom edge).
- On-stage objects passed in glide to their new place, so `place(VGroup(old, new))` gathers them.
- `replaces=old` morphs an on-stage object into the new one (identity carried).
- To choose the entrance yourself, play it right after placing: `self.place(eq)` then
  `self.play(Write(eq))`.
- Refusals are `CompositionError`s, raised before anything moves:
  - "would overlap X": place both in one call, use another region, or start a new beat so X leaves.
  - "needs 0.42x to fit ... below the readable minimum": shorten or split, or use a larger region.
  - "replaces=X is not on stage": X already left; keep it in the beat, or place instead.

## Beats

`with self.beat(id, *, focus=None, transition="continue", keep=(), hold=None, run_time=None,
intent=None, question=None, takeaway=None, aha=False):`

- Entering a beat moves nothing. At its first animation (or its end), one transition happens:
  objects on stage that were not kept or placed again leave, kept and replaced ones glide or morph,
  staged ones enter. Title and caption persist across beats until a `chapter`.
- Transitions: `continue` (default; kept objects glide, the rest fades), `contrast` (old slides out
  as new slides in), `reveal` (new objects are drawn; good for a first beat), `chapter` (clear
  everything, title and caption included).
- `keep=[...]` lists what survives. Anything you `self.play`ed or `self.add`ed in an earlier beat
  leaves unless kept; graphs with updaters (`always_redraw`) need their pieces kept too.
- `focus=obj` dims everything else once `obj` is on stage (it must reach the stage in that beat).
  `run_time` is the transition's length.
- `hold` is the stillness at the end. Leave it unset: the beat then holds until its last change
  and its caption have been read at the viewer's `level`, and at least 2 s (general) after a
  result: `aha=True` or intent `reveal`, `prove` or `recap`. A number sets it exactly.
- `aha=True` marks the one beat where the viewer gets the idea; mirror it in the storyboard. Give
  its key motion a `run_time` of 1.5–3 s.
- Devices and `derive` wait until the last change has been read before they play. A plain
  `self.play`, or a `place` in mid-beat, does not: call `self.pause()` first when it follows a
  reveal.
- The last frame stays still for the viewer's `final_hold` (3 s at general). The class attribute
  `final_hold = 0` turns that off, for a film that loops.
- Beats do not nest. Each beat is a Manim section (`render` with `sections: true` writes one video
  per beat) and a timeline entry with its file and line; QA findings and contact sheets name it.
- Keep the storyboard in `director.yaml` in sync: same ids, in order, one `aha: true`.

## Mathematics

`self.derive(*steps, region="content", in_place=False, notes="auto", run_time=None, pause=None,
replaces=None, min_scale=0.5) -> Derivation`

- Steps are TeX strings or `MathTex`, optionally `(tex, "note")`. The first is written (or morphed
  from `replaces=`), each next line transforms from a copy of the previous one with
  `TransformMatchingTex`, lines stack with their first relation (`=`, `<`, `\le`, ...) in one
  column, so `&` is not needed (a top-level `&` is dropped; continue with `= ...`). Notes are `label`-role text; `$...$` in a note is TeX (`r"divide by $\lambda^n$"`).
  `notes="right"` puts them in a column beside the lines, `"below"` under each line, and `"auto"`
  picks whichever needs less shrinking (below in a 9:16 frame).
- `in_place=True` transforms one line through all steps (notes appear below and swap).
- Leave `pause` unset: after each step the derivation waits for that step's reading time at the
  viewer's `level`.
- Returns a `VGroup` with `.lines` and `.notes`. Continue a derivation in the next beat with
  `derive(..., replaces=steps.lines[-1])`, or promote the result with
  `place(self.math(..., font_size=72), replaces=steps.lines[-1])`.
- Terms match when their atoms are textually identical; keep notation consistent between steps
  (`\frac{b}{2a}` everywhere, not `b/(2a)` in one line) so terms travel instead of fading.

`self.term(eq, r"\frac{b}{2a}", occurrence=None)` returns the glyphs of a sub-term for your own
animations (`self.play(Indicate(self.term(eq, "x^2")))`). `self.highlight(eq, *terms,
color="accent", box=False)` recolors sub-terms (or all of `eq`) and with `box=True` backs each
occurrence with a soft box that moves and leaves with `eq` (`.boxes` on the result); `color=None`
keeps symbol colors and only boxes. `self.tag(eq, label=None)` numbers the equation at the right edge of its region
(`(1)`, `(2)`, ...) and follows it; it raises if `eq` or a derivation note leaves no room. `self.focus(*mobjects)` and
`self.unfocus()` dim and restore everything else.

## Frame shape and vertical video

The frame's aspect ratio comes from `manim.cfg`. A `DirectedScene` whose frame and pixel shapes
differ raises `CompositionError` rather than render squashed. For 9:16, start from
`init(template="vertical_short")` or copy its `manim.cfg` (`pixel_width`, `pixel_height`,
`frame_rate`), portrait `profiles` and the wider `safe_area` for phone overlays.

## Direction

How to plan for a viewer (the viewer model, budgets, beat template, patterns, anti-patterns and
critique) is in [viewer.md](viewer.md). Match motion to meaning:

| Relationship | How |
|---|---|
| Same object, new state | `replaces=`, `derive`, or a kit verb (`apply`, `refine`, `repair`); keep its color. |
| Cause and consequence | A `reveal` beat, or `derive` notes naming the step. |
| Contrast | A `contrast` beat, or `left`/`right` at the same scale with a shared anchor kept. |
| Evidence for a claim | Place it beside the claim; keep the claim. |
| A term and its picture | `link` them; label the picture in the term's color. |
| New chapter | `chapter`, then a new title. |

- Use exact Manim constructions for mathematical content (plots, geometry, data); split plotted
  domains at poles and discontinuities (`FunctionPlot(breaks=...)`); round only for display.
- Seed randomness (`project.seed`), and keep data in files under `data/` or `sources/` so the scene
  reads it instead of hard-coding numbers.

## Starting points

Start from the closest finished film rather than a blank file. The five gallery films are
projects in [`examples/gallery`](../../../examples/gallery) to copy (`director.yaml`, `manim.cfg`,
`scenes/main.py`): each is built on the kit, plans for its viewer, and passes `qa` with no
findings. The five templates come from `init(template=...)` (or `init(scene_template=...)` to add
one to a project); they predate the kit, so write their `brief.viewer` when you adopt one.

| Start from | Pattern | Topic | What it shows |
|---|---|---|---|
| `picture_to_formula` gallery | picture to formula | odd numbers make squares | `DotArray`, `show`, `link`, `derive`, `ask` |
| `concrete_first` gallery | concrete first | a positive test result | `DotArray` selections, `link`, `focus`, `intro` level |
| `contrast` gallery | before and after | shears keep area | `VectorGrid`, `Readout`, `reserve` |
| `misconception` gallery | misconception, then repair | √(a + b) | `misconception`, `Figure`, `link` |
| `zoom_detail` gallery | zoom in on a detail | sin x ≈ x | `FunctionPlot.inset`, `secant`, `Readout` |
| `explainer` template | picture, then algebra | geometric series | regions, `keep=` |
| `derivation` template | step-by-step algebra | quadratic formula | multi-beat `derive`, `replaces`, `tag`, `highlight` |
| `geometry` template | rearranging shapes | Pythagoras | shapes beside an equation |
| `graph` template | a value tracked on a graph | a derivative | `ValueTracker`, `always_redraw` across beats |
| `vertical_short` template | 9:16 for phones | triangular numbers | portrait `manim.cfg`, profiles, safe area |

## Editing existing scenes

- Change only the requested scenes and the coupled files (storyboard, captions, profiles). Preserve
  hand-written code outside that scope.
- Map a request to its layer: "hold the roots longer" is a beat's `hold`; "make it vertical" is a
  `vertical_short`-style config plus portrait profiles, not a crop; "keep my code, fix the camera"
  touches only camera calls.
- Plain Manim scenes need not be converted. Convert to `DirectedScene` only when the user wants its
  layout, beats or derivations; it is a base-class change plus replacing manual positioning.
- Manim Community only: `from manimlib import ...` (ManimGL) projects must be ported by hand, scene
  by scene, rendering each one before moving on.

## Explaining a scene

Explain in execution order: what the viewer sees and why, which beat or lines produce it, then the
few Manim mechanisms involved (mobjects, transforms, trackers, updaters, camera). Read only the
relevant lines; render only if the question is about how it looks.
