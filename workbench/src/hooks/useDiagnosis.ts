import { useEffect, useMemo, useState } from "react";
import type { OpsFinding } from "../api/types.ts";
import { fromDiagnosis, type ShownFinding } from "../model/findings.ts";
import { jobTitle } from "../model/jobs.ts";
import { useStore, useWorkbench } from "../store/useWorkbench.ts";

export interface Diagnosis {
  /** What was diagnosed, e.g. `Render Recurrence · draft`. */
  subject: string;
  findings: ShownFinding[];
}

/**
 * The newest succeeded `diagnose` job's findings, fetched once per job. They
 * read as outdated once a newer job of the same operation and scene succeeds.
 */
export function useNewestDiagnosis(): Diagnosis | null {
  const store = useStore();
  const jobs = useWorkbench((state) => state.jobs);
  const job = jobs.find((candidate) => candidate.operation === "diagnose" && candidate.status === "succeeded") ?? null;
  const [loaded, setLoaded] = useState<{ jobId: string; findings: OpsFinding[] } | null>(null);
  const jobId = job?.id ?? null;
  const loadedId = loaded?.jobId ?? null;

  useEffect(() => {
    if (jobId === null || loadedId === jobId) return;
    let current = true;
    void store.perform(`diagnosis:${jobId}`, (client) => client.job(jobId), ["not_found"]).then((outcome) => {
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
    const outdated = subject !== null && subject.scene_id !== null && jobs.some((candidate) =>
      candidate.scene_id === subject.scene_id
      && candidate.operation === subject.operation
      && candidate.status === "succeeded"
      && candidate.sequence > subject.sequence);
    return {
      subject: jobTitle(subject ?? job),
      findings: fromDiagnosis(job.id, loaded.findings, { sceneId: subject?.scene_id ?? null, outdated }),
    };
  }, [job, jobs, loaded]);
}
