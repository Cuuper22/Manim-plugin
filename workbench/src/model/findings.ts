import type { Finding, JobId, JobSummary, OpsFinding, SceneId, Severity, SourceLocation } from "../api/types.ts";

/** A workspace finding, or one from a `diagnose` job's result. */
export interface ShownFinding extends OpsFinding {
  key: string;
  source: Finding["source"] | "diagnose";
  scene_id: string | null;
  /** The job that reported it. */
  job_id: JobId | null;
  outdated: boolean;
  frame_url: string | null;
}

/** When a finding shows up in a render, and the frame it was measured on. */
export interface Moment {
  at_seconds: number;
  frame_url: string | null;
}

/** Findings that differ only in when they happen (QA samples many frames), shown once. */
export interface FindingCard {
  key: string;
  finding: ShownFinding;
  /** In time order; empty when the finding is not tied to a time. */
  moments: Moment[];
}

export interface SeverityGroup {
  severity: Severity;
  cards: FindingCard[];
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
    job_id: jobId,
    outdated: subject.outdated,
    frame_url: null,
  }));
}

/** Non-empty groups, errors first; repeats are merged and order within a group is kept. */
export function bySeverity(findings: readonly ShownFinding[]): SeverityGroup[] {
  return SEVERITIES.map((severity) => ({ severity, cards: cards(findings.filter((f) => f.severity === severity)) }))
    .filter((group) => group.cards.length > 0);
}

function cards(findings: readonly ShownFinding[]): FindingCard[] {
  const byIdentity = new Map<string, FindingCard>();
  for (const finding of findings) {
    const { location } = finding;
    const identity = JSON.stringify([
      finding.source,
      finding.code,
      finding.message,
      finding.hint,
      finding.scene_id,
      location?.file,
      location?.line,
      finding.outdated,
    ]);
    let card = byIdentity.get(identity);
    if (!card) {
      card = { key: finding.key, finding, moments: [] };
      byIdentity.set(identity, card);
    }
    if (finding.at_seconds !== null) card.moments.push({ at_seconds: finding.at_seconds, frame_url: finding.frame_url });
  }
  const merged = [...byIdentity.values()];
  for (const card of merged) card.moments.sort((a, b) => a.at_seconds - b.at_seconds);
  return merged;
}

/**
 * The selected scene's newest QA when it passed: it found nothing, so there
 * is no finding to show for it. `null` while it runs, failed or found issues.
 */
export function cleanQa(jobs: readonly JobSummary[], findings: readonly ShownFinding[], sceneId: SceneId): JobSummary | null {
  const newest = jobs.find((job) => job.operation === "qa" && job.scene_id === sceneId);
  if (newest?.status !== "succeeded") return null;
  return findings.some((finding) => finding.job_id === newest.id) ? null : newest;
}

/** Where the editor can jump: project-relative files only; host paths stay text. */
export function codeTarget(location: SourceLocation | null): CodeTarget | null {
  if (!location || location.line < 1) return null;
  const { file } = location;
  const isHostPath = file.startsWith("/") || file.startsWith("\\") || /^[A-Za-z]:/.test(file);
  return isHostPath || file === "" ? null : { path: file, line: location.line };
}
