---
name: manim-director
description: Create, edit, explain, render, visually inspect, debug, migrate, and export Manim Community or ManimGL animation projects. Use when the request concerns Manim source, mathematical animation direction, render failures, visual QA, narration timing, or production video delivery.
metadata:
  short-description: Direct production Manim animations
---

# Manim Director

Use the local `manim-director` engine to turn animation intent into editable source and inspected media. Preserve user-authored code, isolate changes to the requested scenes, and prefer semantic edits over wholesale rewrites.

## Route the request

Read only the references needed for the current mode:

- Create or substantially redesign: [project and animation specification](references/project-spec.md), then [authoring and direction](references/authoring.md).
- Edit an existing project: [authoring and direction](references/authoring.md); add [project specification](references/project-spec.md) only when project metadata or output profiles change.
- Render, preview, recover, or optimize: [rendering and debugging](references/render-debug.md) and the relevant entries in [command and tool contract](references/commands.md).
- Diagnose a visual or runtime failure: [rendering and debugging](references/render-debug.md); load [visual QA](references/qa.md) for visible defects.
- Explain code or a project: inspect its inventory and selected scene spans, then use the explanation rules in [authoring and direction](references/authoring.md). Do not render unless visual behavior is part of the question.
- Migrate between Manim CE versions or from ManimGL: [compatibility and migration](references/compatibility.md), plus [visual QA](references/qa.md) for comparison.
- Accept or ship a finished project: [visual QA](references/qa.md) and [BDD acceptance](references/bdd-acceptance.md).

## Reuse the bundled direction assets

For a matching new project, adapt one recipe from `../../assets/recipes/` instead of recreating its beat structure. The copy-ready explainer in `../../assets/templates/explainer/` is a complete runnable baseline, and `../../assets/themes/presets.json` contains the built-in portable theme tokens. Load only the selected recipe or template; do not add all examples to the working project.

## Work from state, not guesses

Start with `project_inspect` for an existing project or `project_init` for a new one. Run `doctor` when rendering is requested, the environment is unknown, or a dependency failure is plausible. Keep the animation spec, source, assets, narration, captions, and output profiles in the project; do not encode persistent decisions only in chat.

For a new animation, establish the objective, audience, rigor level, target duration, output profile, and any supplied mathematical/data sources. Infer ordinary aesthetic defaults. Ask one focused question only if competing interpretations would produce materially different content.

Before writing scene code, choose one visual spine: the object, spatial metaphor, or transformation the viewer will keep recognizing across the film. Plan each beat as a change in the viewer's mental state, not as a list of facts to display. Every beat must name the audience question it answers, its single takeaway, one primary focus, retained context, and the semantic reason for its transition. If a beat needs several unrelated text blocks, split it or replace the prose with a visual operation.

Use `DirectedScene`, `DesignSystem`, and `Beat` for substantial new work. Treat header, content (or its left/right split), and caption lanes as allocated space; do not position independent elements into the same lane and hope z-order hides the conflict. Let `place`/`layout` reflow content or let `beat` replace prior occupants rather than shrinking text below its role minimum. Use `focus` to subordinate retained context. Keep one hero, a small amount of context, and one consistent meaning per color and movement throughout a chapter.

Use `project_apply` with `ingest` paths for supplied notes, data, code, documents, or media so the project receives bounded summaries and a source manifest without placing full source contents in tool output.

## Produce and verify the actual result

For creation, editing, migration, or repair:

1. Write or patch the smallest coherent storyboard/spec/source scope.
2. Run a targeted preview for the changed scene or section.
3. Inspect representative frames and QA findings. A successful Python exit is not visual verification.
4. Repair concrete defects and rerender only invalidated content. Stop after two automatic repair passes unless the user asks to continue.
5. Render the requested production outputs and export source plus reproducibility metadata.

Never call an animation complete without a readable requested artifact and visual inspection of at least its opening state, primary transition or content beat, and final state. For narrated work, also inspect one captioned frame and timing alignment. Report unresolved defects with scene, timestamp, evidence, and consequence.

## Keep context lean

The MCP server intentionally exposes at most ten coarse tools. Prefer those tools over streaming shell logs or reading media directly.

- Request inventories, spans, diagnostics, thumbnails, contact sheets, or artifact paths—not entire projects, full logs, base64 media, or rendered frame sequences.
- Poll jobs with the returned cursor. Reuse the newest cursor and a bounded event limit; do not replay prior pages.
- Use `project_apply` with an expected revision for engine-managed source/spec edits, then target its `affected_scenes` with `preview`. Prefer a line edit or `director.yaml` merge patch over full content; do not resend or reread unchanged source.
- Inspect only the implicated scene, section, source range, or time range.
- Open local images through the host image viewer when visual judgment is required; do not load image bytes into text context.
- Start the workbench with `manim-director serve --port 4177` when the user benefits from timeline, object selection, side-by-side comparison, or live project navigation.

## Preserve correctness and authorship

Do not silently change mathematics, data, units, stated assumptions, narration meaning, or supplied visual identity to make a render easier. Distinguish exact results, numerical approximations, intuition, and conjecture. Validate important values and transformations when the engine can do so.

Do not mix Manim CE and ManimGL APIs in one source tree. Detect the project flavor before editing. Preserve manual code outside the selected scope and keep generated components modular enough for direct human editing.

## Handoff

Lead with what now works. Include output paths or links, the scenes/sections changed, the inspected evidence, and any unresolved blocker. Include setup commands only when needed to run the delivered project. Avoid narrating routine tool calls or dumping validation logs.
