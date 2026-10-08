import { useEffect, useMemo, useState } from "react";
import type { Finding, OpsFinding } from "../api/types.ts";
import { fromDiagnosis, type ShownFinding } from "../model/findings.ts";
import { jobTitle } from "../model/jobs.ts";
import { useStore, useWorkbench } from "../store/useWorkbench.ts";

const NO_FINDINGS: readonly Finding[] = [];

export interface Diagnosis {
  /** What was diagnosed, e.g. `Render Recurrence · draft`. */
  subject: string;
  findings: ShownFinding[];
}

/**
 * The newest succeeded `diagnose` job's findings, fetched once per job. They
 * read as outdated once a newer job of the same operation and scene succeeds,
 * or once the diagnosed job's scene file changed.
 */
export function useNewestDiagnosis(): Diagnosis | null {
  const store = useStore();
  const jobs = useWorkbench((state) => state.jobs);
  const findings = useWorkbench((state) => state.workspace?.findings ?? NO_FINDINGS);
  const job = jobs.find((candidate) => candidate.operation === "diagnose" && candidate.status === "succeeded") ?? null;
  const [loaded, setLoaded] = useState<{ jobId: string; findings: OpsFinding[] } | null>(null);
  const jobId = job?.id ?? null;
  const loadedId = loaded?.jobId ?? null;

  useEffect(() => {
    if (jobId === null || loadedId === jobId) return;
    let current = true;
    void store.perform((client) => client.job(jobId), ["not_found"]).then((outcome) => {
      if (current && outcome.ok && outcome.value.operation === "diagnose" && outcome.value.result) {
        setLoaded({ jobId, findings: outcome.value.result.findings });
      }
    });
    return () => {
      current = false;
    };
  }, [store, jobId, loadedId]);

  return useMemo(() => {
    if (!job || loaded?.jobId !== job.id) return null;
    const subjectId = "job_id" in job.request ? job.request.job_id : null;
    const subject = jobs.find((candidate) => candidate.id === subjectId) ?? null;
    const superseded = subject !== null && subject.scene_id !== null && jobs.some((candidate) =>
      candidate.scene_id === subject.scene_id
      && candidate.operation === subject.operation
      && candidate.status === "succeeded"
      && candidate.sequence > subject.sequence);
    // The engine marks the diagnosed job's own findings outdated once its scene file changed.
    const edited = subject !== null && findings.some((finding) => finding.job_id === subject.id && finding.outdated);
    const outdated = superseded || edited;
    return {
      subject: jobTitle(subject ?? job),
      findings: fromDiagnosis(job.id, loaded.findings, { sceneId: subject?.scene_id ?? null, outdated }),
    };
  }, [job, jobs, findings, loaded]);
}
