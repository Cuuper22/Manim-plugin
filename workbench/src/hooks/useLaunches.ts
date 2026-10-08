import { useCallback, useEffect, useRef, useState } from "react";
import type { JobId, JobSummary, OperationRequest, SceneId } from "../api/types.ts";
import { isActive } from "../model/jobs.ts";
import { useStore, useWorkbench } from "../store/useWorkbench.ts";

/** A job this page started from a button: which button, for which scene. */
export interface Launch {
  action: string;
  sceneId: SceneId | null;
  jobId: JobId;
}

export interface Launches {
  /** Submits `request`; `false` when the engine refused it (already reported). */
  launch: (action: string, sceneId: SceneId | null, request: OperationRequest) => Promise<boolean>;
  /** The newest job this button launched for that scene, while it is listed. */
  jobFor: (action: string, sceneId: SceneId | null) => JobSummary | null;
  /** Its request is being sent, or the job it started has not ended. */
  isBusy: (action: string, sceneId: SceneId | null) => boolean;
}

const keyOf = (action: string, sceneId: SceneId | null) => `${action}:${sceneId ?? ""}`;

/** Tracks the jobs buttons start, and calls `onFinished` once per job when it ends. */
export function useLaunches(onFinished: (launch: Launch, job: JobSummary) => void): Launches {
  const store = useStore();
  const jobs = useWorkbench((state) => state.jobs);
  const [launches, setLaunches] = useState<Readonly<Record<string, Launch>>>({});
  /** Buttons whose request is on its way. */
  const [sending, setSending] = useState<ReadonlySet<string>>(new Set());
  const finished = useRef(new Set<JobId>());
  const notify = useRef(onFinished);
  notify.current = onFinished;

  const launch = useCallback(
    async (action: string, sceneId: SceneId | null, request: OperationRequest) => {
      const key = keyOf(action, sceneId);
      setSending((current) => new Set(current).add(key));
      const outcome = await store.submit(request);
      setSending((current) => new Set([...current].filter((sent) => sent !== key)));
      if (outcome.ok) setLaunches((current) => ({ ...current, [key]: { action, sceneId, jobId: outcome.value.id } }));
      return outcome.ok;
    },
    [store],
  );

  useEffect(() => {
    for (const started of Object.values(launches)) {
      if (finished.current.has(started.jobId)) continue;
      const job = jobs.find((candidate) => candidate.id === started.jobId);
      if (!job || isActive(job)) continue;
      finished.current.add(started.jobId);
      notify.current(started, job);
    }
  }, [jobs, launches]);

  const jobFor = useCallback(
    (action: string, sceneId: SceneId | null) => {
      const started = launches[keyOf(action, sceneId)];
      return started ? jobs.find((job) => job.id === started.jobId) ?? null : null;
    },
    [jobs, launches],
  );

  const isBusy = (action: string, sceneId: SceneId | null) => {
    const job = jobFor(action, sceneId);
    return sending.has(keyOf(action, sceneId)) || (job !== null && isActive(job));
  };

  return { launch, jobFor, isBusy };
}
