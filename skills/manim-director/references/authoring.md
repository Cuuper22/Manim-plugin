# Authoring with DirectedScene

Read this before writing or substantially changing a scene. Everything here is plain Manim
underneath: helpers return ordinary mobjects, and any Manim code mixes in.

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

```python
with self.beat("roots", transition="continue", keep=[plane], focus=None, hold=1.0,
               run_time=None, intent="prove", question="...", takeaway="..."):
    ...
```

- Entering a beat moves nothing. At its first animation (or its end), one transition happens:
  objects on stage that were not kept or placed again leave, kept and replaced ones glide or morph,
  staged ones enter. Title and caption persist across beats until a `chapter`.
- Transitions: `continue` (default; kept objects glide, the rest fades), `contrast` (old slides out
  as new slides in), `reveal` (new objects are drawn; good for a first beat), `chapter` (clear
  everything, title and caption included).
- `keep=[...]` lists what survives. Anything you `self.play`ed or `self.add`ed in an earlier beat
  leaves unless kept; graphs with updaters (`always_redraw`) need their pieces kept too.
- `focus=obj` dims everything else once `obj` is on stage (it must reach the stage in that beat).
  `hold` is the pause at the end (default 1 s); `run_time` the transition's length.
- Beats do not nest. Each beat is a Manim section (`render` with `sections: true` writes one video
  per beat) and a timeline entry with its file and line; QA findings and contact sheets name it.
- Keep the storyboard in `director.yaml` in sync: same ids, in order.

## Mathematics

`self.derive(*steps, region="content", in_place=False, run_time=None, pause=None, replaces=None,
min_scale=0.5) -> Derivation`

- Steps are TeX strings or `MathTex`, optionally `(tex, "note")`. The first is written (or morphed
  from `replaces=`), each next line transforms from a copy of the previous one with
  `TransformMatchingTex`, lines stack with their first relation (`=`, `<`, `\le`, ...) in one
  column, notes sit to the right in the `label` role.
- `in_place=True` transforms one line through all steps (notes appear below and swap).
- Returns a `VGroup` with `.lines` and `.notes`. Continue a derivation in the next beat with
  `derive(..., replaces=steps.lines[-1])`, or promote the result with
  `place(self.math(..., font_size=72), replaces=steps.lines[-1])`.
- Terms match when their atoms are textually identical; keep notation consistent between steps
  (`\frac{b}{2a}` everywhere, not `b/(2a)` in one line) so terms travel instead of fading.

`self.term(eq, r"\frac{b}{2a}", occurrence=None)` returns the glyphs of a sub-term for your own
animations (`self.play(Indicate(self.term(eq, "x^2")))`). `self.highlight(eq, *terms,
color="accent", box=False)` recolors sub-terms (or all of `eq`) and with `box=True` adds a soft
backing box. `self.tag(eq, label=None)` numbers the equation at the right edge of `content`
(`(1)`, `(2)`, ...); it raises if `eq` is too wide to leave room. `self.focus(*mobjects)` and
`self.unfocus()` dim and restore everything else.

## Frame shape and vertical video

The frame's aspect ratio comes from `manim.cfg`. A `DirectedScene` whose frame and pixel shapes
differ raises `CompositionError` rather than render squashed. For 9:16, start from
`init(template="vertical_short")` or copy its `manim.cfg` (`pixel_width`, `pixel_height`,
`frame_rate`), portrait `profiles` and the wider `safe_area` for phone overlays.

## Direction

A strong explanation changes the viewer's mind one step at a time:

1. Pose the question or the surprising fact.
2. Give the viewer one concrete object to track.
3. Change one thing per beat, keeping that object on stage.
4. State the rule after the viewer has seen evidence for it.
5. Stress it with an edge case or counterexample.
6. Resolve the opening question on a stable final frame.

Match motion to meaning:

| Relationship | How |
|---|---|
| Same object, new state | `replaces=` or `derive`; keep its color. |
| Cause and consequence | A `reveal` beat, or `derive` notes naming the step. |
| Contrast | A `contrast` beat, or `left`/`right` with a shared anchor kept. |
| Evidence for a claim | Place it beside the claim; keep the claim. |
| New chapter | `chapter`, then a new title. |

- One hero per beat; dim context with `focus` rather than deleting it.
- Prefer a picture next to its algebra (`left`/`right`) to text explaining the picture.
- Give dense formulas reading time (`hold`, `pause`); speed is not energy.
- Colors carry meaning: one meaning per color across the film, set once in `symbols`.
- Use exact Manim constructions for mathematical content (plots, geometry, data); split plotted
  domains at poles and discontinuities; round only for display.
- Seed randomness (`project.seed`), and keep data in files under `data/` or `sources/` so the scene
  reads it instead of hard-coding numbers.

## Starting points

`init(scene_template=...)` adds a finished scene to read and adapt: `explainer` (picture, then
algebra, regions, `keep=`), `derivation` (multi-beat `derive`, `replaces`, `tag`, `highlight`),
`geometry` (rearranging shapes beside an equation), `graph` (`ValueTracker` and `always_redraw`
readouts kept across beats), `vertical_short` (9:16). Copy their patterns rather than inventing
new layout code.

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
