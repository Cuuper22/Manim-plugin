import type { Artifact, JobOperation, JobSummary, OperationRequest, Progress, SceneId } from "../api/types.ts";

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

export function statusText(job: JobSummary): string {
  switch (job.status) {
    case "queued":
      return job.cancel_requested ? "Cancelling" : "Queued";
    case "running": {
      if (job.cancel_requested) return "Cancelling";
      const progress = job.progress;
      if (!progress) return "Running";
      const fraction = progressFraction(progress);
      return fraction === null ? `Running · ${progress.phase}` : `Running · ${progress.phase} ${Math.round(fraction * 100)}%`;
    }
    case "succeeded":
      return job.cached ? "Done (cached)" : "Done";
    case "failed":
      return "Failed";
    case "cancelled":
      return "Cancelled";
  }
}

const DELIVERABLE_KINDS = new Set<Artifact["kind"]>(["archive", "video", "image", "contact_sheet"]);

/** What an export hands over: the first archive or media file it produced. */
export function deliverable(job: JobSummary): Artifact | null {
  if (job.operation !== "export" || job.status !== "succeeded") return null;
  return job.artifacts.find((artifact) => DELIVERABLE_KINDS.has(artifact.kind)) ?? null;
}
