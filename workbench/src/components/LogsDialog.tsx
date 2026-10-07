import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { JobId, JobSummary, LogEntry } from "../api/types.ts";
import { clockTime } from "../model/format.ts";
import { isActive, jobTitle, retryRequest, statusText } from "../model/jobs.ts";
import { useStore, useWorkbench } from "../store/useWorkbench.ts";
import { JobProgress } from "./Progress.tsx";

const PAGE_SIZE = 200;
const LEVEL_CLASS: Record<LogEntry["level"], string | undefined> = { info: undefined, warning: "warning", error: "danger" };
const TAIL_MS = 1000;
/** Older lines beyond this are dropped from view; the engine keeps them. */
const MAX_ENTRIES = 10_000;

interface LogsDialogProps {
  jobId: JobId | null;
  onClose: () => void;
  onDiagnose: (job: JobSummary) => void;
}

/** A job's log, paged from the start and tailed (≤ 1 Hz) while the job runs. */
export function LogsDialog({ jobId, onClose, onDiagnose }: LogsDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (jobId && !dialog.current?.open) dialog.current?.showModal();
    if (!jobId) dialog.current?.close();
  }, [jobId]);
  return (
    <dialog ref={dialog} className="dialog" aria-labelledby="logs-title" onClose={onClose}>
      {jobId ? <LogsBody key={jobId} jobId={jobId} onDiagnose={onDiagnose} /> : null}
    </dialog>
  );
}

function LogsBody({ jobId, onDiagnose }: { jobId: JobId; onDiagnose: (job: JobSummary) => void }) {
  const store = useStore();
  const job = useWorkbench((state) => state.jobs.find((candidate) => candidate.id === jobId) ?? null);
  const [entries, setEntries] = useState<readonly LogEntry[]>([]);
  /** The last page was full: more lines are waiting to be paged in. */
  const [more, setMore] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const cursor = useRef<string | undefined>(undefined);
  const fetching = useRef(false);
  /** Asked for while a fetch ran, which may have been answered before the lines that prompted it. */
  const refetch = useRef(false);
  const list = useRef<HTMLOListElement>(null);
  const pinned = useRef(true);

  const fetchPage = useCallback(async () => {
    if (fetching.current) {
      refetch.current = true;
      return;
    }
    fetching.current = true;
    do {
      refetch.current = false;
      const page = { after: cursor.current, limit: PAGE_SIZE };
      const outcome = await store.perform(`logs:${jobId}`, (client) => client.jobLogs(jobId, page));
      if (!outcome.ok) break;
      const { items, next_after } = outcome.value;
      cursor.current = items.at(-1)?.cursor ?? cursor.current;
      if (items.length > 0) setEntries((current) => [...current, ...items].slice(-MAX_ENTRIES));
      setMore(next_after !== null);
      setLoaded(true);
    } while (refetch.current);
    fetching.current = false;
  }, [store, jobId]);

  const active = job !== null && isActive(job);
  useEffect(() => {
    void fetchPage();
  }, [fetchPage, active]);
  useEffect(() => {
    if (!active || more) return;
    const timer = setInterval(() => void fetchPage(), TAIL_MS);
    return () => clearInterval(timer);
  }, [active, more, fetchPage]);

  useLayoutEffect(() => {
    if (pinned.current && list.current) list.current.scrollTop = list.current.scrollHeight;
  }, [entries]);

  const title = job ? jobTitle(job) : "Job";
  const retry = job ? retryRequest(job) : null;
  return (
    <>
      <header className="bar">
        <div>
          <h2 id="logs-title">{title}</h2>
          {job ? <p className="muted">{statusText(job)}</p> : null}
        </div>
        <div className="row">
          {job && active ? (
            <button type="button" disabled={job.cancel_requested} onClick={() => void store.cancel(job.id)}>
              Cancel
            </button>
          ) : null}
          {job && retry ? (
            <button type="button" onClick={() => void store.submit(`retry:${job.id}`, retry)}>
              Retry
            </button>
          ) : null}
          {job?.status === "failed" && job.operation !== "diagnose" ? (
            <button type="button" onClick={() => onDiagnose(job)}>
              Diagnose
            </button>
          ) : null}
          <form method="dialog">
            <button type="submit">Close</button>
          </form>
        </div>
      </header>
      {job?.status === "running" ? <JobProgress job={job} label={`${title} progress`} /> : null}
      {job?.error ? (
        <p className="card" data-tone="danger" role="alert">
          {job.error.message} <span className="muted">({job.error.code})</span>
        </p>
      ) : null}
      <ol
        ref={list}
        className="log mono"
        aria-label="Log lines"
        tabIndex={0}
        onScroll={(event) => {
          const box = event.currentTarget;
          pinned.current = box.scrollHeight - box.scrollTop - box.clientHeight < 24;
        }}
      >
        {entries.map((entry) => (
          <li key={entry.cursor} data-level={entry.level}>
            <time className="muted" dateTime={entry.timestamp}>{clockTime(entry.timestamp, true)}</time>
            <span className="muted">{entry.stream}</span>
            <span className={LEVEL_CLASS[entry.level]}>{entry.message}</span>
          </li>
        ))}
      </ol>
      {loaded && entries.length === 0 ? <p className="pane-note">No log lines{active ? " yet" : ""}.</p> : null}
      {more ? (
        <button type="button" className="quiet" onClick={() => void fetchPage()}>
          Load more lines
        </button>
      ) : null}
    </>
  );
}
