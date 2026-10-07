# Verifying an animation

Read this when reviewing renders, accepting a fix, or deciding whether a deliverable is done.

## What to look at

For every changed scene, look at:

- the opening state once the first beat has settled;
- each beat's main transformation, and both sides of a changed transition;
- the final frame;
- for captioned or narrated work, one frame per caption change.

Start with `contact_sheet` (each tile is labelled with its time and beat); raise `count` for long
scenes. Grab exact moments with `frame` at `at_seconds` taken from the render's timeline
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
`beat`, the beat's `file:line` and the sampled frame's path. `qa` does not see overlaps inside the
frame, formula legibility, timing or correctness; a `pass` only means none of the three problems
were found in the sampled frames. Open the finding's frame before changing anything: a sample taken
mid-transition can be legitimately faint.

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
- [ ] `qa` has no errors; every warning was looked at and either fixed or explained.
- [ ] Every derivation's algebra passed `validate_math` with honest `ranges` for its assumptions,
      or the handoff says which steps were not checkable and why.
- [ ] The storyboard ids in `director.yaml` match the scene's beats, in order.
- [ ] Mathematics, data and claims match what the user supplied; approximations and intuitions are
      labelled as such.
- [ ] Captions, if any, pass `captions` validation and their cue times match the render timeline.
- [ ] Requested exports exist (`output/` by default) and the handoff lists their paths.
- [ ] Anything unresolved is reported with scene, time, evidence and consequence.
