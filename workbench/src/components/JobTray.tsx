import { useEffect, useRef, useState } from "react";
import type { JobId, JobStatus, JobSummary } from "../api/types.ts";
import { useDismiss } from "../hooks/useShortcuts.ts";
import { deliverable, isActive, jobTitle, progressFraction, retryRequest, statusText } from "../model/jobs.ts";
import { downloadUrl } from "../model/media.ts";
import { useStore, useWorkbench } from "../store/useWorkbench.ts";
import { Icon } from "./Icon.tsx";
import { Progress } from "./Progress.tsx";

const PAGE = 25;

const DOT_STATES: Record<JobStatus, string> = {
  queued: "running",
  running: "running",
  succeeded: "rendered",
  failed: "failed",
  cancelled: "none",
};

interface JobTrayProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onLogs: (jobId: JobId) => void;
  onDiagnose: (job: JobSummary) => void;
}

/** Every job of the project, newest first, live from the event stream. */
export function JobTray({ open, onOpenChange, onLogs, onDiagnose }: JobTrayProps) {
  const jobs = useWorkbench((state) => state.jobs);
  const root = useRef<HTMLDivElement>(null);
  const running = jobs.filter(isActive).length;
  const announcement = useAnnouncement(jobs);
  useDismiss(open, () => onOpenChange(false), root);

  return (
    <div className="menu-anchor" ref={root}>
      <button type="button" aria-expanded={open} aria-controls={open ? "job-tray" : undefined} onClick={() => onOpenChange(!open)}>
        Jobs
        {running > 0 ? <span className="muted"> · {running} active</span> : null}
      </button>
      <p className="visually-hidden" aria-live="polite">
        {announcement}
      </p>
      {open ? <TrayPanel jobs={jobs} running={running} onClose={() => onOpenChange(false)} onLogs={onLogs} onDiagnose={onDiagnose} /> : null}
    </div>
  );
}

interface TrayPanelProps extends Pick<JobTrayProps, "onLogs" | "onDiagnose"> {
  jobs: readonly JobSummary[];
  running: number;
  onClose: () => void;
}

function TrayPanel({ jobs, running, onClose, onLogs, onDiagnose }: TrayPanelProps) {
  const store = useStore();
  const hasOlder = useWorkbench((state) => state.jobsNextBefore !== null);
  const [shown, setShown] = useState(PAGE);
  const visible = jobs.slice(0, shown);
  return (
    <section className="popover tray" id="job-tray" aria-labelledby="job-tray-title">
      <header className="row tray-header">
        <h2 id="job-tray-title">Jobs</h2>
        <span className="meta">{running > 0 ? `${running} active` : "None running"}</span>
        <button type="button" className="quiet small icon" aria-label="Close" onClick={onClose}>
          <Icon name="close" />
        </button>
      </header>
      {visible.length === 0 ? <p className="pane-note">No jobs yet. Preview a scene to start one.</p> : null}
      <ul>
        {visible.map((job) => (
          <JobRow key={job.id} job={job} onLogs={onLogs} onDiagnose={onDiagnose} />
        ))}
      </ul>
      {shown < jobs.length ? (
        <button type="button" className="quiet" onClick={() => setShown(shown + PAGE)}>
          Show more
        </button>
      ) : hasOlder ? (
        <button type="button" className="quiet" onClick={() => void store.loadOlderJobs().then(() => setShown(shown + PAGE))}>
          Load older jobs
        </button>
      ) : null}
    </section>
  );
}

function JobRow({ job, onLogs, onDiagnose }: { job: JobSummary } & Pick<JobTrayProps, "onLogs" | "onDiagnose">) {
  const store = useStore();
  const title = jobTitle(job);
  const retry = retryRequest(job);
  const download = deliverable(job);
  return (
    <li className="job" data-status={job.status}>
      <div className="row">
        <span className="dot" data-state={DOT_STATES[job.status]} aria-hidden="true" />
        <span className="job-title">{title}</span>
        <time className="meta" dateTime={job.created_at}>
          {new Date(job.created_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </time>
      </div>
      {job.status === "running" ? <Progress fraction={progressFraction(job.progress)} label={`${title} progress`} /> : null}
      {job.error && job.status === "failed" ? <p className="meta danger clamp">{job.error.message}</p> : null}
      <div className="row job-actions">
        <span className="meta job-status">{statusText(job)}</span>
        <button type="button" className="quiet small" onClick={() => onLogs(job.id)}>
          Logs
        </button>
        {isActive(job) ? (
          <button type="button" className="quiet small" disabled={job.cancel_requested} onClick={() => void store.cancel(job.id)}>
            Cancel
          </button>
        ) : null}
        {retry ? (
          <button type="button" className="quiet small" onClick={() => void store.submit(`retry:${job.id}`, retry)}>
            Retry
          </button>
        ) : null}
        {job.status === "failed" && job.operation !== "diagnose" ? (
          <button type="button" className="quiet small" onClick={() => onDiagnose(job)}>
            Diagnose
          </button>
        ) : null}
        {download ? (
          <a className="button small" href={downloadUrl(download)} download>
            <Icon name="export" />
            Download
          </a>
        ) : null}
      </div>
    </li>
  );
}

/** One line for screen readers when jobs start or end; jobs listed on load or paged in are not announced. */
function useAnnouncement(jobs: readonly JobSummary[]): string {
  const seen = useRef<{ statuses: Map<JobId, JobStatus>; newest: number } | null>(null);
  const [message, setMessage] = useState("");
  useEffect(() => {
    const first = seen.current === null;
    const known = (seen.current ??= { statuses: new Map(), newest: 0 });
    const lines: string[] = [];
    for (const job of jobs) {
      const before = known.statuses.get(job.id);
      known.statuses.set(job.id, job.status);
      const isNew = before === undefined && job.sequence > known.newest;
      if (!first && (isNew || (before !== undefined && before !== job.status && !isActive(job)))) {
        lines.push(`${jobTitle(job)}: ${statusText(job)}`);
      }
    }
    known.newest = Math.max(known.newest, ...jobs.map((job) => job.sequence));
    if (lines.length > 0) setMessage(lines.join(". "));
  }, [jobs]);
  return message;
}
