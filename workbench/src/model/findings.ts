import type { Finding, JobId, OpsFinding, SceneId, Severity, SourceLocation } from "../api/types.ts";

/** A workspace finding, or one from a `diagnose` job's result. */
export interface ShownFinding extends OpsFinding {
  key: string;
  source: Finding["source"] | "diagnose";
  scene_id: string | null;
  outdated: boolean;
  frame_url: string | null;
}

export interface SeverityGroup {
  severity: Severity;
  findings: ShownFinding[];
}

export interface CodeTarget {
  path: string;
  line: number;
}

const SEVERITIES: readonly Severity[] = ["error", "warning", "info"];

export function fromWorkspace(findings: readonly Finding[]): ShownFinding[] {
  return findings.map(({ id, ...finding }) => ({ ...finding, key: id }));
}

/** `subject`: the diagnosed job's scene, and whether a successful job has since superseded it. */
export function fromDiagnosis(
  jobId: JobId,
  findings: readonly OpsFinding[],
  subject: { sceneId: SceneId | null; outdated: boolean },
): ShownFinding[] {
  return findings.map((finding, index) => ({
    ...finding,
    key: `diagnose:${jobId}:${index}`,
    source: "diagnose",
    scene_id: subject.sceneId,
    outdated: subject.outdated,
    frame_url: null,
  }));
}

/** Non-empty groups, errors first; order within a group is kept. */
export function bySeverity(findings: readonly ShownFinding[]): SeverityGroup[] {
  return SEVERITIES.map((severity) => ({ severity, findings: findings.filter((f) => f.severity === severity) }))
    .filter((group) => group.findings.length > 0);
}

/** Where the editor can jump: project-relative files only; host paths stay text. */
export function codeTarget(location: SourceLocation | null): CodeTarget | null {
  if (!location || location.line < 1) return null;
  const { file } = location;
  const isHostPath = file.startsWith("/") || file.startsWith("\\") || /^[A-Za-z]:/.test(file);
  return isHostPath || file === "" ? null : { path: file, line: location.line };
}
