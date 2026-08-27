# Compatibility and migration

Read this reference before changing Manim flavor, upgrading a pinned Manim version, or repairing renderer/version-specific behavior.

## Detect before editing

Determine the project flavor from imports, configuration, lockfiles, and a narrow source inventory. `from manim import ...` indicates Community Edition; `from manimlib import ...` usually indicates ManimGL. If signals conflict, report the mixed state and establish a single target before writing source.

Never mix CE and ManimGL imports in one executable scene. Isolate unavoidable dual support behind separate adapters and entrypoints.

## Migration record

Record:

- source flavor and exact installed/pinned version;
- target flavor and version;
- selected renderer and platform;
- incompatible constructs found;
- semantic replacements applied;
- behavior intentionally changed;
- representative before/after artifacts and QA result.

Keep the original branch/files recoverable. Do not overwrite a working source project before a target preview renders.

## Migration workflow

1. Inventory scene classes, custom mobjects, plugins, renderer-specific code, text engines, assets, configuration, and output profiles.
2. Produce a mechanical compatibility map without changing creative intent.
3. Patch shared configuration/imports and one representative scene.
4. Render and compare semantic keyframes.
5. Migrate remaining scenes by incompatibility family, reusing validated adapters.
6. Render all requested profiles and complete visual QA.
7. Pin the new dependency set and export a migration report.

## Constructs that require judgment

- Camera and frame APIs, especially moving/3D cameras.
- `CONFIG` dictionaries versus constructor/class configuration.
- Scene embedding, interactive preview, and OpenGL-only behavior.
- Updater signatures, lifecycle, and suspension semantics.
- Text/LaTeX object tokenization and matching transforms.
- Shader/material features, lighting, depth ordering, and z-index.
- CLI/config keys, quality flags, section rendering, and output paths.
- Third-party Manim plugins and custom subclasses that depend on internals.

Use target-version behavior observed in a narrow render. Do not blindly rename APIs across an entire project.

## Renderer portability

When a scene must work under both Cairo and OpenGL, keep the semantic scene logic shared and place renderer differences in small helpers. Confirm object bounds, opacity, layering, camera framing, and 3D depth under each requested renderer. A fallback is acceptable only when renderer-specific features are not part of the requested result.

## Version upgrades within CE

Prefer the smallest supported version jump that resolves the user's goal. Update dependency pins and configuration together. Repair deprecations by documented semantics, then render representative scenes; an import-only check misses many camera, transform, and typography changes.

## ManimGL to CE

Treat this as a behavioral port, not a search-and-replace. Preserve visual intent, timing, and mathematical meaning while translating project structure, camera behavior, shader-dependent visuals, interactive constructs, and configuration. If CE lacks an exact capability, identify the closest faithful implementation and disclose the difference before propagating it across scenes.

## Acceptance

A migration is complete when the target environment starts cleanly, all inventoried scenes are discoverable, requested outputs render, registered claims/data remain correct, representative before/after keyframes have no unexplained semantic differences, and the new project is reproducible from its lock/config files.
