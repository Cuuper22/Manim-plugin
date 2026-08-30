# Authoring and direction

Read this reference to create, substantially edit, or explain scenes. Prefer ordinary Manim primitives and compact reusable components; add abstractions only when they remove real repetition or isolate renderer/version differences.

## Direct a change in the viewer, not a sequence of slides

A strong explanatory sequence usually has these beats:

1. Establish the question or surprising behavior.
2. Give the viewer a concrete object to track.
3. Change one idea at a time and preserve visual continuity.
4. State the general rule after the viewer has evidence for it.
5. Stress the rule with an edge case or counterexample.
6. Resolve the opening question and leave a stable final frame.

This is a palette, not a mandatory six-part template. Skip beats that do not help the requested experience.

For each beat, record this compact contract before writing Manim code:

- `intent`: what this beat does in the argument—introduce, explain, compare, reveal, prove, or recap;
- `audience_question`: the question now alive in the viewer's mind;
- `takeaway`: the one new inference the viewer should be able to make when the beat ends;
- `focus`: the single hero object, plus any retained context that keeps it meaningful;
- `visual_metaphor`: the stable visual model being extended, if one is in use;
- `transition`: why the next state follows—continuation, contrast, reveal, or chapter; and
- `max_active`: the maximum number of independently attention-seeking objects on screen.

Also record entering/exiting objects, narration/caption cue, and approximate duration. A beat is overloaded when its takeaway needs "and" to join unrelated claims, when two objects both need to be the hero, or when prose explains a relationship the image could perform. Split it or redesign the visual operation. Titles, labels, and captions orient the viewer; they do not substitute for the explanation.

## Establish a visual language once

Choose a small, semantic system before the first scene and reuse it across chapters:

- one visual spine the viewer can keep recognizing;
- one meaning per accent color, spatial direction, and recurring shape;
- one type family with role-based sizes rather than per-object font choices;
- one spacing scale, safe frame, stroke hierarchy, and corner treatment; and
- one motion grammar that makes transformations predictable without making them monotonous.

Use at most three prominent text sizes in one frame. The hero may be large; supporting labels must be visibly subordinate. Do not make every statement a card, surround every object with a container, or decorate empty space. Repetition should build recognition. Variation should mark a genuine change in meaning.

## Scene and component boundaries

- Use a scene for a coherent chapter that can render independently.
- Use named sections for previewable beats inside a scene.
- Keep data preparation, symbolic computation, layout, and animation direction separable.
- Put reusable visual behavior in components; keep one-off choreography close to its scene.
- Give meaningful mobjects semantic names. Avoid index-heavy manipulation whose meaning breaks after a small edit.
- Centralize theme tokens, safe areas, typography, and renderer compatibility helpers.
- Use `ValueTracker` and updaters for genuinely continuous relationships. Remove or suspend updaters when their beat ends.
- Seed randomness and make time-dependent simulations reproducible.

## Mathematical and data integrity

- Validate algebraic equivalence for important equation morphs when symbolic checking is available.
- Use an explicit token map for transformations whose visual correspondence matters. Fall back to a fade/replace when token topology is ambiguous.
- Sample plotted functions before animation. Split domains at poles, holes, branch cuts, or other discontinuities.
- Keep exact values exact in calculations; round only at the display boundary and state the display rule.
- Verify displayed summaries against source data. Keep units attached through conversion and label axes with units.
- Distinguish proof, derivation, numerical evidence, intuition, and conjecture in both narration and imagery.
- Add focused assertions for important values. Do not create tests whose only job is to confirm the scene contains a particular string.

## Compose through an allocated stage

- Reserve a safe margin from every output edge. Include platform overlays when targeting vertical social video.
- Allocate header, content (or its non-overlapping left/right split), and caption lanes before placing objects. A lane has one owner unless the beat explicitly declares an overlay relationship.
- Establish a clear primary object. De-emphasize retained context rather than deleting it without cause.
- Register independent objects with the stage so layout can preserve gaps, reflow a row into a column, or retire a previous occupant. Do not solve collisions by incidental z-order or by repeatedly calling `shift`.
- Keep captions in their reserved lane and fit the content region above it. A caption must never cover the visual evidence it describes.
- Prefer replacing the prior beat's hero in place over accumulating completed explanations around the frame.
- When content does not fit, simplify, wrap within its region, change the layout, or split the beat. Shrinking below the type role's readable minimum is not a layout strategy.
- Give dense formulas and novel diagrams enough reading time; speed is not the same as energy.
- Keep z-order intentional. Avoid depending on incidental creation order when overlaps are meaningful.
- Use moving cameras sparingly and verify the final camera frame at section boundaries.

Use motion to express the relationship between states:

| Meaning | Direction |
|---|---|
| Same object, changed state | Morph or travel continuously; preserve identity and color role. |
| Cause produces consequence | Use a `reveal`; draw the consequence from, along, or immediately after the cause. |
| Contrast | Keep a shared anchor and swap or separate the differing parts. |
| Added evidence | Introduce beside the claim while retaining the referenced object as context. |
| Scope change within a chapter | Continue with a restrained camera move or scale change, then restabilize the frame. |
| New chapter | Use a `chapter` reset or crossfade and deliberately establish a new visual spine. |

Decorative motion must not compete with the current inference. Avoid unexplained teleportation, a parade of unrelated entrance effects, and camera moves used merely to add energy. Creativity comes from a precise visual metaphor and revealing it well—not from maximizing animation variety.

### Use the direction runtime as the default

For substantial new Manim CE scenes, import the direction primitives from `manim_director_runtime` and subclass `DirectedScene`; use `DirectedMovingCameraScene` or `DirectedThreeDScene` only when the visual model needs that camera. Construct `DesignSystem` from the project's `direction` mapping. Keep the scene source ordinary Python and drop down to native Manim whenever a custom construction needs it.

- Create copy and formula objects through `styled_text` and `styled_math` so `TypeScale` roles—not ad hoc numbers—control hierarchy.
- Use `place` for a named `Region` and `layout` for related objects. These return explicit placements before motion begins.
- Describe the mental-state change with `Beat`, then stage it with `self.beat(beat, *mobjects, keys=(...), region="content", flow="column")`. The method applies the transition, replaces or retains stage occupants, and records the current audience-state beat.
- Use `focus`/`release_focus` to retain context without letting it compete. Use `caption` for the reserved caption lane.
- Use `clear_stage` for a real chapter break, not between every sentence. Continuations and causal steps should preserve the identity of objects the viewer is tracking.

`CompositionLayout` prevents independent placements from occupying the same stage space and prefers fitting, reflow, or a clear layout error over silent overlap. The layout engine is a baseline; the author still chooses the meaningful visual metaphor, hierarchy, and reveal.

## Text and formulas

Use Pango text for ordinary copy, `MathTex`/`Tex` for LaTeX mathematics, and Typst only when the selected runtime supports it. Detect fonts before final layout and define a portable fallback. Wrap prose to an allocated region rather than shrinking it below the minimum readable size.

For right-to-left or mixed-direction text, render a representative production-resolution frame early. Treat localization as layout work: translated strings may require new line breaks, positions, timing, and camera framing.

## Assets, narration, and captions

- Keep original assets in `assets/`; normalize derivatives in the cache.
- Preserve SVG aspect ratio and view box; recolor through theme tokens when appropriate.
- Record source/attribution for third-party assets.
- Prefer exact native diagrams for mathematical information. Generated imagery may illustrate, but must not encode exact values or geometry.
- Make narration cues drive beat holds. Do not accelerate speech to rescue an overloaded scene.
- Export SRT/VTT from the same cue timeline used by the animation.
- Avoid overlapping cues and keep burned captions inside the profile safe area.
- Ensure the silent version still communicates the core sequence through labels and visual continuity.

## Natural-language edits

Resolve an edit into one of: storyboard, source, theme, timing, narration, caption, profile, or asset. Patch all coupled representations, but no others. Examples:

- “Hold the roots longer” changes the relevant cue/beat duration and downstream timestamps.
- “Make it vertical” adds or updates a responsive output/layout variant; it does not crop 16:9.
- “Keep my code, fix the camera” limits changes to camera and any unavoidable framing helpers.
- “Replace circles with vectors” updates the semantic component and affected transforms, not unrelated scenes.

After applying a semantic patch, preview only the invalidated scenes or sections plus their transition boundaries.

## Explain mode

Explain at the user's altitude and in execution order:

1. What the viewer sees and why it matters.
2. Which scene/section creates that behavior.
3. The few Manim mechanisms responsible—mobjects, transforms, trackers, updaters, or camera.
4. Any surprising lifecycle, renderer, or timing behavior.

Use project inventory and narrow source spans. Do not dump the full source or turn a simple explanation into an API catalog.
