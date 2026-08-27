# Rendering and debugging

Read this reference for preview, production render, recovery, performance work, or any failure whose cause is not already obvious.

## Render pipeline

The engine follows a resumable pipeline:

1. Inspect the project and resolve the scene/section/profile target.
2. Run environment checks relevant to that target.
3. Compute an input fingerprint from source, config, assets, runtime versions, and seed.
4. Execute Manim in an isolated worker with time, memory, output, path, and optional network limits.
5. Index produced artifacts and logs without returning media bytes through MCP.
6. Extract representative frames/contact sheets and run requested QA profiles.
7. Reuse valid intermediates for unchanged scenes and assemble the requested final output.

Preview uses the same source and dependency graph as production at a cheaper profile. A preview is evidence about composition and timing, not final codec fidelity.

## Target narrowly

Render the smallest unit that can answer the question:

- Changed object timing: section plus boundary frames.
- Scene-local code repair: scene preview.
- Theme or font change: representative scenes, then full output after approval or when already requested.
- Audio/caption change: affected cue range plus final mux.
- Dependency/runtime migration: representative comparison, then all scenes.
- Final delivery: requested full output profile.

Use the job cursor for progress. Request only new events, and fetch a log span around the first actionable error. Do not replay a full log on every poll.

## Diagnosis order

| Symptom | Inspect first | Likely repair |
|---|---|---|
| Scene is not discoverable | entrypoint, class name, flavor/version | correct inventory/import path |
| Python traceback | first project frame in traceback and narrow source span | patch responsible logic |
| LaTeX/Typst failure | failing expression and compiler excerpt | fix syntax/template/package |
| Missing glyph/font | resolved font inventory and fallback | bundle/select supported font |
| Missing asset | normalized project-relative path | restore asset or approved fallback |
| Wrong equation morph | source/target token topology | explicit token map or safer transition |
| Plot bridges a pole | sampled domain and discontinuities | split plotted intervals |
| Blank/near-blank frames | camera, opacity, z-order, object bounds | repair state or framing |
| Clipped/tiny content | production-profile bounds and safe area | reflow/reframe, then rerender |
| Flicker/one-frame artifact | adjacent frames around timestamp | fix lifecycle/updater/z-order |
| Slow render | per-scene timing, object count, updaters, vector complexity | cache/static rasterize/simplify responsibly |
| Memory growth | retained mobjects/updaters and frame cache | release/suspend, split scene, bound cache |
| Cairo/OpenGL mismatch | compatibility inventory | renderer-specific helper or documented fallback |
| Audio drift | cue timeline, sample rate, mux timestamps | reconcile cues and remux |

Fix the earliest causal error, not every downstream message. Preserve healthy scenes.

## Repair loop

1. Record the concrete defect and affected scope.
2. Apply one coherent repair.
3. Rerender only that scope.
4. Inspect the same evidence that exposed the defect.
5. Stop when resolved or after the configured automatic pass limit.

If the defect remains, return the scene/section, timestamp or source span, cause, attempted repairs, and current artifact. Do not hide a failed final render behind a successful preview.

## Renderer and format fallback

Use the requested renderer when supported. A fallback is allowed only when the scene does not depend on renderer-specific behavior and the visual result is verified. Record it in the render report.

Validate container/codec capabilities before expensive work. Alpha output requires a format/codec that preserves alpha; GIF does not preserve audio; a still frame has no duration or audio. Do not silently discard requested channels.

## Performance without feature loss

- Cache immutable computations and normalized assets by content fingerprint.
- Render independent scenes concurrently within CPU, memory, and I/O budgets.
- Reuse unchanged scene outputs and remux instead of rerendering animation frames.
- Suspend inactive updaters and avoid recreating invariant mobjects per frame.
- Precompute numerical trajectories when they do not depend on interactive state.
- Use spatially appropriate vector detail; preserve exact geometry where the visual claim depends on it.
- Keep production resolution/FPS at the requested values. Optimization must not silently degrade the deliverable.

## Completion conditions

A job succeeds only when the process exits successfully, every requested artifact exists, each artifact is readable and matches its profile, and required QA evidence was produced. An import check, zero exit code with no media, stale cached output, or uninspected render is not completion.
