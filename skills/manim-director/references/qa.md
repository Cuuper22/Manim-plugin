# Visual and content QA

Read this reference when inspecting previews, accepting a repair or migration, or shipping final media. QA is evidence-driven and scoped; it is not a giant ceremonial checklist.

## Minimum inspection set

For each changed scene, inspect:

- the opening stable state;
- the primary reveal, transform, or simulation state;
- the closing stable state;
- both sides of any changed scene transition;
- one captioned frame and one cue boundary when narration/captions are present;
- both eyes or representative camera angles for stereoscopic/3D outputs when applicable.

Use representative frames or a contact sheet first. Scrub a narrow time range when motion, flicker, occlusion, or synchronization cannot be judged from stills.

## Automated findings

The engine may flag:

- bounds outside the frame or safe area;
- collisions between protected text/formula regions;
- text below profile minimum size;
- insufficient contrast;
- blank or near-blank frames;
- unexpected large frame-to-frame changes;
- z-fighting or near-coplanar 3D surfaces;
- missing/extra duration, stream, resolution, FPS, codec, or alpha;
- caption overlap, unsafe placement, invalid order, or excessive line density;
- mismatches between declared and rendered scene/section duration;
- discrepancies between displayed values and registered assertions.

Automated findings guide inspection; they do not replace it. A large intentional transition can look like flicker, and bounding boxes do not prove that a formula is understandable.

## Human-visible acceptance

Judge the rendered output, not source intent:

- The primary subject is obvious without narration.
- Important states remain visible long enough to read.
- Object identity is preserved across meaningful transforms.
- No object disappears, teleports, clips, or occludes another without purpose.
- Camera moves land on stable, correctly framed states.
- Equations, plots, labels, and values convey the intended claim accurately.
- The final frame resolves the scene rather than ending mid-motion.
- Audio is intelligible, appropriately leveled, and synchronized.
- Captions are readable, correctly timed, and do not cover essential visuals.

## Accessibility

- Meaning cannot rely on color alone; add shape, texture, labels, or motion cues.
- Check contrast against the rendered background, including semi-transparent layers.
- Respect profile-specific minimum text sizes and safe areas.
- Avoid avoidable rapid flashing. When intense motion is requested, provide a reduced-motion variant if needed.
- Include captions for spoken content and a plain transcript when requested.
- For audio-described output, place descriptions in genuine pauses or extend the beat.

## Mathematical and data QA

Confirm the exact claims registered in the project spec, not every incidental implementation detail. Verify:

- equation transformations under stated assumptions;
- function domains/discontinuities and axis labels;
- units, rounding, and significant figures;
- table/chart values against the source data;
- geometry constraints that the proof or explanation depends on;
- consistency across aspect-ratio and localization variants.

When validation cannot establish a claim, label it appropriately rather than presenting it as proved.

## Migration and comparison

Compare semantic keyframes, not raw pixel hashes. Allow antialiasing, font rasterization, and renderer noise. Flag changes in layout, object presence, camera framing, timing, color/opacity, data values, or transform meaning. Inspect every flagged semantic difference before accepting the migration.

## QA result contract

Each finding contains severity (`blocker`, `major`, `minor`, `info`), scene/section, timestamp or frame, rule, concise evidence, and artifact/frame reference. A pass records the inspected evidence and profile. Keep thumbnails and reports as files; return paths and summaries through MCP.

A production artifact cannot pass with unresolved blockers. Majors require an explicit user decision only when the requested creative intent makes automatic repair ambiguous. Minors may ship when disclosed and non-material.
