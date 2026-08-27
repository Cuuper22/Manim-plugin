import type { WorkspaceState } from "../types";

const sceneColors = ["#298bd0", "#16a49b", "#2cad69", "#7852a3"];

const waveform = Array.from({ length: 180 }, (_, index) => {
  const carrier = Math.sin(index * 1.91) * 0.28 + Math.sin(index * 0.37) * 0.47;
  const envelope = 0.28 + Math.abs(Math.sin(index * 0.081)) * 0.72;
  return Math.max(0.08, Math.min(1, Math.abs(carrier) * envelope + ((index * 17) % 13) / 55));
});

export const demoWorkspace: WorkspaceState = {
  projectId: "generalized-fibonacci",
  projectName: "Generalized Fibonacci",
  duration: 44,
  fps: 30,
  scenes: [
    { id: "hook", name: "Hook", start: 0, end: 9, file: "scenes/hook.py", color: sceneColors[0], enabled: true },
    { id: "recurrence", name: "Recurrence", start: 9, end: 20, file: "scenes/recurrence.py", color: sceneColors[1], enabled: true },
    { id: "roots", name: "Characteristic Roots", start: 20, end: 34, file: "scenes/roots.py", color: sceneColors[2], enabled: true },
    { id: "recap", name: "Recap", start: 34, end: 44, file: "scenes/recap.py", color: sceneColors[3], enabled: true },
  ],
  beats: [
    { id: "intro", sceneId: "hook", name: "Intro", start: 0, end: 2.9, kind: "beat", source: { file: "scenes/hook.py", line: 26, object: "title_group" } },
    { id: "show-sequence", sceneId: "hook", name: "Show Sequence", start: 2.9, end: 7.8, kind: "beat", source: { file: "scenes/hook.py", line: 62, object: "sequence_dots" } },
    { id: "reveal-recurrence", sceneId: "recurrence", name: "Reveal Recurrence", start: 7.8, end: 16.4, kind: "beat", source: { file: "scenes/recurrence.py", line: 91, object: "recurrence_eq" } },
    { id: "solve-roots", sceneId: "roots", name: "Solve Roots", start: 16.4, end: 24.3, kind: "beat", source: { file: "scenes/roots.py", line: 114, object: "root_plane" } },
    { id: "closed-form", sceneId: "roots", name: "Closed Form", start: 24.3, end: 32.1, kind: "beat", source: { file: "scenes/roots.py", line: 146, object: "closed_form" } },
    { id: "interpretation", sceneId: "roots", name: "Interpretation", start: 32.1, end: 39.8, kind: "beat", source: { file: "scenes/roots.py", line: 181, object: "growth_curve" } },
    { id: "summary", sceneId: "recap", name: "Summary", start: 39.8, end: 44, kind: "beat", source: { file: "scenes/recap.py", line: 48, object: "summary" } },
    { id: "title", sceneId: "hook", name: "Title", start: 0.2, end: 3.7, kind: "visual" },
    { id: "seq-growth", sceneId: "hook", name: "Sequence Growth", start: 3.7, end: 8.7, kind: "visual" },
    { id: "question", sceneId: "hook", name: "?", start: 8.7, end: 9.8, kind: "visual" },
    { id: "equation", sceneId: "recurrence", name: "Equation", start: 9.8, end: 14.3, kind: "visual" },
    { id: "values", sceneId: "recurrence", name: "Values", start: 14.3, end: 20.8, kind: "visual" },
    { id: "graph-morph", sceneId: "roots", name: "Graph Morph", start: 20.8, end: 26.6, kind: "visual" },
    { id: "roots-plane", sceneId: "roots", name: "Roots Plane", start: 26.6, end: 32.9, kind: "visual" },
    { id: "formula", sceneId: "roots", name: "Formula", start: 32.9, end: 37.8, kind: "visual" },
    { id: "wrap", sceneId: "recap", name: "Wrap Up", start: 37.8, end: 44, kind: "visual" },
    { id: "cap-1", sceneId: "hook", name: "Consider a sequence where each term remembers the two before it.", start: 0.25, end: 8.2, kind: "caption" },
    { id: "cap-2", sceneId: "recurrence", name: "Each term is a weighted sum, controlled by p and q.", start: 8.3, end: 16.2, kind: "caption" },
    { id: "cap-3", sceneId: "recurrence", name: "This leads to a family of familiar and surprising sequences.", start: 16.35, end: 23.1, kind: "caption" },
    { id: "cap-4", sceneId: "roots", name: "The characteristic roots explain the long-run growth.", start: 23.2, end: 31.0, kind: "caption" },
    { id: "cap-5", sceneId: "roots", name: "Thus we get an exact closed form.", start: 31.1, end: 36.7, kind: "caption" },
    { id: "cap-6", sceneId: "recap", name: "So the sequence is geometry hiding inside recurrence.", start: 36.8, end: 41.5, kind: "caption" },
    { id: "cap-7", sceneId: "recap", name: "In summary: choose p, q, and two seeds.", start: 41.6, end: 44, kind: "caption" },
  ],
  assets: [
    { id: "voice", name: "narration.wav", type: "audio", size: "7.4 MB", path: "assets/audio/narration.wav" },
    { id: "logo", name: "director-mark.svg", type: "svg", size: "3.1 KB", path: "assets/director-mark.svg" },
    { id: "paper", name: "sequence-reference.pdf", type: "image", size: "1.8 MB", path: "assets/references/sequence-reference.pdf" },
    { id: "font", name: "STIXTwoMath.otf", type: "font", size: "613 KB", path: "assets/fonts/STIXTwoMath.otf" },
  ],
  selection: {
    id: "seq-value-7",
    name: "SeqValue_7",
    type: "Dot",
    visible: true,
    locked: false,
    position: [7, 46.652, 0],
    scale: [0.8, 0.8, 0.8],
    rotation: 0,
    anchor: "CENTER",
    color: "#ff6b5e",
    fillOpacity: 1,
    stroke: "#ff6b5e",
    strokeWidth: 2,
    appear: 2.9,
    start: 2.9,
    end: 3.4,
    fadeIn: 0.1,
    fadeOut: 0.1,
    easing: "smooth",
    source: { file: "scenes/hook.py", line: 128, object: "values_dots[7]" },
  },
  sourceCode: `from manim import *
from math_helpers import generalized_sequence
from colors import DIRECTOR_BLUE, DIRECTOR_CORAL

class Hook(Scene):
    def construct(self):
        p, q = 1.618, 1.0
        values = generalized_sequence(p, q, seeds=(1, 1), count=9)

        title = Text("Generalized Fibonacci", font_size=48)
        recurrence = MathTex(
            r"u_n", "=", r"p u_{n-1}", "+", r"q u_{n-2}"
        ).next_to(title, DOWN, aligned_edge=LEFT)

        axes = Axes(
            x_range=[0, 8, 1], y_range=[0, 80, 10],
            x_length=9.5, y_length=3.2,
            tips=False,
        )
        values_dots = VGroup(*[
            Dot(axes.c2p(index, value), color=interpolate_color(
                DIRECTOR_BLUE, DIRECTOR_CORAL, index / 8
            ))
            for index, value in enumerate(values)
        ])

        curve = VMobject().set_points_smoothly([
            dot.get_center() for dot in values_dots
        ]).set_stroke(DIRECTOR_CORAL, width=3)

        self.play(Write(title), Write(recurrence))
        self.play(Create(axes), LaggedStartMap(GrowFromCenter, values_dots))
        self.play(Create(curve), run_time=2.2)
        self.wait(0.8)
`,
  logs: [
    { id: "l1", time: "12:14:22", level: "INFO", message: "Render completed successfully in 00:06.28 (182 frames @ 30 fps)", source: "scenes/hook.py:1-210" },
    { id: "l2", time: "12:14:22", level: "INFO", message: "Wrote output to media/videos/hook/480p15/Hook.mp4", source: "renderer.py:312" },
    { id: "l3", time: "12:13:58", level: "WARNING", message: "Dot stroke width 0 may not be visible at small scales.", source: "mobject/geometry.py:467" },
    { id: "l4", time: "12:13:58", level: "INFO", message: "Render started: Hook (Preview 720p, Cairo)", source: "renderer.py:165" },
  ],
  renderQueue: [
    { id: "render-hook", scene: "Hook", profile: "Preview 720p", renderer: "Cairo", status: "complete", progress: 100, frames: 182, totalFrames: 182, output: "media/videos/hook/Hook.mp4" },
    { id: "render-roots", scene: "Characteristic Roots", profile: "Production 1080p", renderer: "Cairo", status: "queued", progress: 0, frames: 0, totalFrames: 420 },
  ],
  exports: [
    { id: "exp-1", name: "generalized-fibonacci-preview.mp4", format: "MP4", size: "12.7 MB", createdAt: "12:14" },
    { id: "exp-2", name: "generalized-fibonacci.en.vtt", format: "VTT", size: "4.2 KB", createdAt: "12:14" },
  ],
  camera: Array.from({ length: 20 }, (_, index) => ({
    time: index * (44 / 19),
    value: 0.48 + Math.sin(index * 0.75) * 0.16 + (index % 3) * 0.025,
  })),
  waveform,
};

export function freshDemoWorkspace(): WorkspaceState {
  return structuredClone(demoWorkspace);
}
