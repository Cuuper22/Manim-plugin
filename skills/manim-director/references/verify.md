# Verifying an animation

Read this when reviewing renders, accepting a fix, or deciding whether a deliverable is done.

## What to look at

For every changed scene, look at:

- `qa`'s `beats.png`: each beat's settled frame under its audience question, and the final frame
  under the film's question. Run the frames-only critique in
  [viewer.md](viewer.md#7-frames-only-critique) on it;
- each beat's main transformation, and both sides of a changed transition;
- for captioned or narrated work, one frame per caption change.

Then `contact_sheet` for the flow (each tile is labelled with its time and beat); raise `count`
for long scenes. Grab exact moments with `frame` at `at_seconds` taken from the render's timeline
(`<Scene>.timeline.json` lists each beat's start and end) or from a `qa` finding. Use `still` for the
final layout without rendering video.

Judge the frames, not the code:

- One clear subject per beat; the eye knows where to look without narration.
- Formulas are fully on screen, readable at the delivery resolution, and not overlapping labels,
  axes or each other.
- Terms that should travel between derivation steps travel (matching notation), rather than
  fading out and back in.
- Colors keep one meaning across the film; symbol colors are consistent.
- Objects do not pop, teleport or linger from an earlier beat by accident (check `keep=`).
- Every state that has to be read stays on screen long enough (`hold`, `pause`).
- The last frame is a resolved, stable state.

## What the automatic checks catch

`qa` samples `frames` frames and measures pixels:

| Finding | Severity | Trigger |
|---|---|---|
| `blank_frame` | error | The frame is uniform or nearly so. |
| `low_contrast` | warning | Content against the background is below 3:1. |
| `safe_area` | warning | Content reaches past the `safe_area` margins or crowds the frame edge. |

`status` is `fail` with any error, `warn` with warnings, else `pass`. Findings carry `at_seconds`,
`beat`, a `file:line` and the sampled frame's path. Open the finding's frame before changing
anything: a sample taken mid-transition can be legitimately faint.

## Pacing and viewer findings

For a `DirectedScene` render, `qa` also reads the beat timeline and judges it against the viewer's
budgets (`brief.viewer.level`; numbers in [viewer.md](viewer.md#3-budgets)). Each finding names
the statement that caused it. None is an error: fix every warning, and fix or answer every info
finding in the handoff.

| Code | Severity | Fires when | Hint |
|---|---|---|---|
| `caption_too_fast` | warning | a caption leaves before max(`caption_min_seconds`, `read_base_text` + words ÷ `words_per_second`) | Call self.pause() before the next caption or change, or use fewer words. |
| `motion_while_reading` | warning | the scene's own play starts moving while the newest caption is still being read | Call self.pause() before the motion: it waits until the caption has been read. |
| `short_hold` | warning | the stage changes again before the last reveal could be read, or a beat ends with less than `beat_end_min` (`result_end_min` for a result beat) of stillness | Call self.pause() before the next change (it waits as long as the viewer needs), or drop an explicit hold= or pause=. |
| `question_hold_short` | warning | something moves less than `ask_hold` after an `ask` | Keep everything still while the viewer predicts: leave ask's hold at its default. |
| `rushed_step` | warning | a derive step or morph is shorter than `motion_min`, a derive pause than `step_pause_min`, too many glyphs change at once, or the aha's motion is shorter than `aha_motion_min` | Raise run_time= or pause=; give the aha its own slow motion. |
| `crowded_beat` | warning | more than `max_new_per_beat` new chunks enter one beat | Split the beat, or reveal its parts across beats. |
| `crowded_moment` | warning | one animation moves more than `max_targets_per_motion` things, or things in two places | Play them one after the other; use link() for a deliberate pair. |
| `text_overlap` | warning | two visible texts overlap at a still | Place them in one call, or in different regions. |
| `long_text` | info | a caption is over `caption_max_words` words or a title over `title_max_words` | Split it across beats; let labels on the picture do the naming. |
| `long_beat` | info | a beat lasts over `beat_max_seconds` | Split it: one takeaway per beat. |
| `unsignaled_reveal` | info | a beat brings in `signal_min_chunks` or more with no caption change, focus, highlight, note, link or ask | Caption it, or focus, highlight or annotate the one that matters. |
| `too_dense` | info | more than `max_visible_chunks` lit chunks at a still | Let old objects leave (drop them from keep=), or focus the one that matters. |
| `palette_overload` | info | more than `max_colors` meaningful colors at a still | Give fewer things their own color; context can be muted. |
| `unexplained_notation` | info | a symbol-colored token or a mark (right angle, equal ticks) appears before anything names it, unless `knows` lists it as `$…$` | Name it once where it appears: a label on the picture, a caption or a note. |
| `recap_without_replay` | info | in a film over `plan_min_seconds` with one aha, the last beat does not replay the aha's longest motion | Write the aha as a method and call it again, faster, in the last beat. |
| `viewer_plan` | info | the brief lacks `question`, `wrong_guess` or `aha`; a beat lacks an audience question or takeaway; storyboard and beats differ; or a film over `plan_min_seconds` lacks an `ask` or exactly one aha | Fill brief.viewer and the storyboard (see the skill's template). |

`qa` also writes `beats.png` (a `contact_sheet` artifact labelled `beats`). It cannot tell whether
a frame explains anything: that is the frames-only critique, and it is yours.

`DirectedScene` itself refuses overlapping placements and unreadable scales when the scene runs
(a `composition` finding), so most layout problems surface as render failures with a line number.

`validate_math` checks that consecutive steps are equal:

- Steps are Python-syntax expressions (`^` or `**`, explicit `*`, functions `sqrt exp log ln log10
  sin cos tan asin acos atan sinh cosh tanh abs floor ceil min max`, constants `pi e tau`), not
  LaTeX and not equations. Write each equation as `lhs - rhs`, divided by any factor the step
  applied to both sides, so that consecutive steps are equal: `ax^2 + bx + c = 0` then
  `x^2 + (b/a)x = -c/a` is `["(a*x^2 + b*x + c)/a", "x^2 + b/a*x + c/a"]`. Adding the same term to
  both sides needs nothing. A step that changes the solutions (squaring both sides, a `±` root) has
  no such form: substitute each solution into the earlier equation's `lhs - rhs` and check it
  against `"0"`, or report the step as not checkable.
- SymPy proves equality when it can; otherwise values are compared at `samples` random points
  (seeded) in -10..10 per variable, or in `ranges`. A range with a lower bound ≥ 0 also tells SymPy
  the variable is nonnegative (`sqrt(x^2)` equals `x` only with `ranges: {"x": [0, 5]}`).
- `valid: false` comes with the failing pair, the symbolic difference and a counterexample. `valid:
  null` means it could not decide: every sample was outside the domain, or one step is defined where
  the other is not (the counterexample's `left` or `right` is null there; narrow `ranges`).
- For sums with `\cdots`, check a concrete instance:
  `["(1 - r^4)/(1 - r)", "1 + r + r^2 + r^3"]` with `ranges: {"r": [-0.9, 0.9]}`.

## Acceptance checklist

A deliverable is done when all of these hold:

- [ ] `doctor` reports the project ready to render.
- [ ] Every requested scene rendered at the requested profile without failure, and the artifacts
      exist with the requested resolution, frame rate and format (see `media` in the result).
- [ ] You looked at the opening, the main transformations and the final frame of each changed
      scene, and at a captioned frame when there are captions.
- [ ] `qa` has no errors and no pacing warnings; every other warning and every info finding was
      fixed or answered.
- [ ] `beats.png` passes the frames-only test: each tile answers its audience question, and the
      final frame shows the question and its answer.
- [ ] Every derivation's algebra passed `validate_math` with honest `ranges` for its assumptions,
      or the handoff says which steps were not checkable and why.
- [ ] Mathematics, data and claims match what the user supplied; approximations and intuitions are
      labelled as such.
- [ ] Captions, if any, pass `captions` validation and their cue times match the render timeline.
- [ ] Requested exports exist (`output/` by default) and the handoff lists their paths.
- [ ] Anything unresolved is reported with scene, time, evidence and consequence.
