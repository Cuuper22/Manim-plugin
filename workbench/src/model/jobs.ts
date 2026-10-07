import type { Artifact, JobOperation, JobSummary, OperationRequest, Progress, SceneId } from "../api/types.ts";
import { formatTime } from "./format.ts";

const OPERATION_LABELS: Record<JobOperation, string> = {
  doctor: "Doctor",
  render: "Render",
  still: "Still",
  frame: "Frame",
  contact_sheet: "Contact sheet",
  qa: "QA",
  diagnose: "Diagnose",
  validate_math: "Validate math",
  captions: "Captions",
  ingest: "Ingest",
  export: "Export",
};

/** `Recurrence` from `scenes/main.py#Recurrence`. */
export function sceneName(sceneId: SceneId): string {
  return sceneId.slice(sceneId.lastIndexOf("#") + 1);
}

/** `Render SequenceData · draft`, `Export · gif`. */
export function jobTitle(job: JobSummary): string {
  const subject = job.scene_id ? ` ${sceneName(job.scene_id)}` : "";
  const detail = job.request.operation === "export" ? job.request.format ?? "zip" : job.profile;
  return `${OPERATION_LABELS[job.operation]}${subject}${detail ? ` · ${detail}` : ""}`;
}

export function isActive(job: JobSummary): boolean {
  return job.status === "queued" || job.status === "running";
}

/** Retried by re-posting `job.request` verbatim, which HTTP accepts for every operation but `ingest`. */
export function retryRequest(job: JobSummary): OperationRequest | null {
  if (job.status !== "failed" && job.status !== "cancelled") return null;
  return job.request.operation === "ingest" ? null : job.request;
}

/** Done fraction in `[0, 1]`, or `null` while indeterminate. */
export function progressFraction(progress: Progress | null): number | null {
  if (!progress || progress.total === null || progress.total <= 0) return null;
  return Math.min(Math.max(progress.current / progress.total, 0), 1);
}

/** `animate 40%`, or how far into the scene a render of unknown length got: `animate at 0:10.03`. */
export function progressDetail(progress: Progress): string {
  const fraction = progressFraction(progress);
  if (fraction !== null) return `${progress.phase} ${Math.round(fraction * 100)}%`;
  return progress.scene_seconds === null ? progress.phase : `${progress.phase} at ${formatTime(progress.scene_seconds)}`;
}

export function statusText(job: JobSummary): string {
  switch (job.status) {
    case "queued":
      return job.cancel_requested ? "Cancelling" : "Queued";
    case "running":
      if (job.cancel_requested) return "Cancelling";
      return job.progress ? `Running · ${progressDetail(job.progress)}` : "Running";
    case "succeeded":
      return job.cached ? "Done (cached)" : "Done";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Cancelled";
  }
}

/** The operations the stage starts for its scene. */
const STAGE_OPERATIONS = new Set<JobOperation>(["render", "still", "frame", "contact_sheet", "qa", "export"]);

const ACTIVITY: Partial<Record<JobOperation, string>> = {
  render: "Rendering",
  still: "Rendering the last frame",
  frame: "Grabbing a frame",
  contact_sheet: "Laying out a contact sheet",
  qa: "Checking frames",
  export: "Exporting",
};

/**
 * What the stage reports for a scene: its newest stage job while it is
 * queued or running, or when it failed and nothing has run for the scene since.
 */
export function stageActivity(jobs: readonly JobSummary[], sceneId: SceneId): JobSummary | null {
  const newest = jobs.find((job) => job.scene_id === sceneId && STAGE_OPERATIONS.has(job.operation));
  return newest && (isActive(newest) || newest.status === "failed") ? newest : null;
}

/** `Rendering · draft`, `Export failed`. */
export function activityText(job: JobSummary): string {
  if (job.status === "failed") return `${OPERATION_LABELS[job.operation]} failed`;
  const detail = job.request.operation === "export" ? job.request.format ?? "zip" : job.profile;
  return `${ACTIVITY[job.operation] ?? OPERATION_LABELS[job.operation]}${detail ? ` · ${detail}` : ""}`;
}

const DELIVERABLE_KINDS = new Set<Artifact["kind"]>(["archive", "video", "image", "contact_sheet"]);

/** What an export hands over: the first archive or media file it produced. */
export function deliverable(job: JobSummary): Artifact | null {
  if (job.operation !== "export" || job.status !== "succeeded") return null;
  return job.artifacts.find((artifact) => DELIVERABLE_KINDS.has(artifact.kind)) ?? null;
}
