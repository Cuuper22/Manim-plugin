# Authoring and direction

Read this reference to create, substantially edit, or explain scenes. Prefer ordinary Manim primitives and compact reusable components; add abstractions only when they remove real repetition or isolate renderer/version differences.

## Design the explanation before the motion

A strong explanatory sequence usually has these beats:

1. Establish the question or surprising behavior.
2. Give the viewer a concrete object to track.
3. Change one idea at a time and preserve visual continuity.
4. State the general rule after the viewer has evidence for it.
5. Stress the rule with an edge case or counterexample.
6. Resolve the opening question and leave a stable final frame.

This is a palette, not a mandatory six-part template. Skip beats that do not help the requested experience.

For each beat, record its purpose, visible objects, entering/exiting objects, primary focus, narration/caption cue, and approximate duration. If several unrelated facts compete on one frame, split the beat.

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

## Composition and motion

- Reserve a safe margin from every output edge. Include platform overlays when targeting vertical social video.
- Establish a clear primary object. De-emphasize retained context rather than deleting it without cause.
- Give dense formulas and novel diagrams enough reading time; speed is not the same as energy.
- Use consistent movement semantics: matching objects travel or morph; conceptual replacement fades or swaps; camera movement signals a scope change.
- Keep z-order intentional. Avoid depending on incidental creation order when overlaps are meaningful.
- Use moving cameras sparingly and verify the final camera frame at section boundaries.
- Favor transforms that preserve identity. Avoid unexplained teleportation and decorative motion that competes with the idea.

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
