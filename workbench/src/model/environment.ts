import type { DoctorSnapshot, SceneIndexStatus } from "../api/types.ts";

export interface EnvironmentStatus {
  text: string;
  /** `danger` when the check failed, rendering is impossible or the scene index failed. */
  tone: "danger" | null;
}

/** One line on the newest environment check and, when its scan failed, the scene index. */
export function environmentStatus(doctor: DoctorSnapshot | null, index: SceneIndexStatus): EnvironmentStatus {
  const parts: string[] = [];
  let failed = false;
  if (!doctor) {
    parts.push("Environment not checked yet");
  } else if (doctor.error) {
    parts.push(`Environment check failed (${doctor.error.code})`);
    failed = true;
  } else {
    const { report } = doctor;
    const manim = report.checks.find((check) => check.name === "manim")?.version;
    const missing = [
      !report.capabilities.render && "rendering",
      !report.capabilities.latex && "LaTeX",
      !report.capabilities.video_tools && "video tools",
    ].filter(Boolean);
    parts.push(`Python ${report.runtime.python}`, manim ? `Manim ${manim}` : "Manim missing");
    if (missing.length > 0) parts.push(`missing ${missing.join(", ")}`);
    failed = !report.ok;
  }
  if (index.state === "failed") {
    parts.push(`Scene index failed (${index.error?.code ?? "unknown error"})`);
    failed = true;
  }
  return { text: parts.join(" · "), tone: failed ? "danger" : null };
}
