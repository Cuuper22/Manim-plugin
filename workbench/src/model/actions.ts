import type { ExportFormat, OperationRequest, Scene, SceneLatest } from "../api/types.ts";

/** The stage's job buttons (CONTRACT-http §9). */
export type StageAction = "preview" | "render" | "still" | "frame" | "contact_sheet" | "qa";

/** Previews always render with this built-in profile. */
export const PREVIEW_PROFILE = "draft";

export interface ActionContext {
  scene: Scene | null;
  latest: SceneLatest | null;
  /** The profile picker's choice. */
  profile: string;
  /** Seconds, read when the action runs. */
  playhead: number;
}

/** What a button submits, or why it cannot. */
export type Plan = { ok: true; request: OperationRequest } | { ok: false; reason: string };

const NO_SCENE: Plan = { ok: false, reason: "Select a scene first." };
const NO_VIDEO: Plan = { ok: false, reason: "Needs a rendered video of this scene. Preview it first." };

export function planAction(action: StageAction, context: ActionContext): Plan {
  const { scene, latest, profile } = context;
  if (!scene) return NO_SCENE;
  const target = { scene: scene.class_name, file: scene.file };
  const video = latest?.video ?? null;
  switch (action) {
    case "preview":
      return { ok: true, request: { operation: "render", ...target, profile: PREVIEW_PROFILE } };
    case "render":
      return { ok: true, request: { operation: "render", ...target, profile } };
    case "still":
      return { ok: true, request: { operation: "still", ...target, profile } };
    case "frame": {
      if (!video) return NO_VIDEO;
      const at = clampToDuration(context.playhead, video.artifact.media?.duration_seconds ?? null);
      return { ok: true, request: { operation: "frame", source: { job_id: video.job_id }, at_seconds: at } };
    }
    case "contact_sheet":
      if (!video) return NO_VIDEO;
      return { ok: true, request: { operation: "contact_sheet", source: { job_id: video.job_id } } };
    case "qa": {
      const checked = video ?? latest?.still ?? null;
      if (!checked) return { ok: false, reason: "Needs a render or a still of this scene first." };
      return { ok: true, request: { operation: "qa", source: { job_id: checked.job_id } } };
    }
  }
}

/** Media formats deliver the latest video; `zip` bundles the project, with that video when there is one. */
export function planExport(format: ExportFormat, latest: SceneLatest | null): Plan {
  const video = latest?.video ?? null;
  if (!video) return format === "zip" ? { ok: true, request: { operation: "export", format } } : NO_VIDEO;
  return { ok: true, request: { operation: "export", format, source: { job_id: video.job_id } } };
}

/** Whole milliseconds inside `[0, duration]`, so repeated grabs of one frame hit the cache. */
function clampToDuration(seconds: number, duration: number | null): number {
  const upper = duration ?? Number.POSITIVE_INFINITY;
  return Math.round(Math.min(Math.max(seconds, 0), upper) * 1000) / 1000;
}
