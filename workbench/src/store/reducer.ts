import type { AppliedEvent } from "../api/events.ts";
import type {
  EngineInfo,
  ErrorBody,
  JobPage,
  JobStatus,
  JobSummary,
  Progress,
  WorkspaceSections,
  WorkspaceState,
} from "../api/types.ts";

/** Newer jobs push the oldest out; older ones can be paged back in. */
export const MAX_JOBS = 500;
const MAX_TOASTS = 4;

export type Connection =
  | { status: "connecting" }
  | { status: "online" }
  /** The event stream dropped and is reconnecting; data may be stale. */
  | { status: "reconnecting" }
  /** The engine cannot be used; `reason` says why. Reconnects on its own. */
  | { status: "disconnected"; reason: string }
  /** The session is gone (e.g. the engine restarted) until the engine's new link is opened in this browser. */
  | { status: "unauthorized" };

export interface Workspace extends WorkspaceSections {
  engine: EngineInfo;
}

export interface ActionState {
  pending: boolean;
  error: ErrorBody | null;
}

export interface Toast {
  id: number;
  code: string;
  message: string;
}

export interface WorkbenchState {
  connection: Connection;
  /** The last project root seen; kept while disconnected to name the command that reconnects. */
  root: string | null;
  workspace: Workspace | null;
  /** Newest first (by `sequence`). */
  jobs: readonly JobSummary[];
  jobsNextBefore: string | null;
  /** Revisions announced by `file` events; `null` = deleted. */
  fileRevisions: Readonly<Record<string, string | null>>;
  /** Counts snapshots (start, resync, reconnect): events may have been missed before each. */
  snapshots: number;
  /** Absent key = idle. */
  actions: Readonly<Record<string, ActionState>>;
  toasts: readonly Toast[];
  nextToastId: number;
}

export type StoreAction =
  | { type: "snapshot"; state: WorkspaceState }
  | { type: "event"; event: AppliedEvent }
  | { type: "job"; job: JobSummary }
  | { type: "older_jobs"; page: JobPage }
  | { type: "connection"; connection: Connection }
  | { type: "action_started"; key: string }
  | { type: "action_succeeded"; key: string }
  | { type: "action_failed"; key: string; error: ErrorBody; toast: boolean }
  | { type: "error_reported"; error: ErrorBody }
  | { type: "toast_dismissed"; id: number };

export const initialState: WorkbenchState = {
  connection: { status: "connecting" },
  root: null,
  workspace: null,
  jobs: [],
  jobsNextBefore: null,
  fileRevisions: {},
  snapshots: 0,
  actions: {},
  toasts: [],
  nextToastId: 1,
};

export function reduce(state: WorkbenchState, action: StoreAction): WorkbenchState {
  switch (action.type) {
    case "snapshot": {
      const { engine, event_cursor: _cursor, jobs, jobs_next_before, ...sections } = action.state;
      return {
        ...state,
        connection: { status: "online" },
        root: sections.project.root,
        workspace: { ...sections, engine },
        jobs,
        jobsNextBefore: jobs_next_before,
        snapshots: state.snapshots + 1,
      };
    }
    case "event":
      return applyEvent(state, action.event);
    case "job":
      return withJob(state, action.job);
    case "older_jobs": {
      const known = new Set(state.jobs.map((job) => job.id));
      const older = action.page.items.filter((job) => !known.has(job.id));
      return capJobs({ ...state, jobs: bySequence([...state.jobs, ...older]), jobsNextBefore: action.page.next_before });
    }
    case "connection":
      return sameConnection(state.connection, action.connection) ? state : { ...state, connection: action.connection };
    case "action_started":
      return { ...state, actions: { ...state.actions, [action.key]: { pending: true, error: null } } };
    case "action_succeeded": {
      const { [action.key]: _done, ...actions } = state.actions;
      return { ...state, actions };
    }
    case "action_failed": {
      const failed = { ...state, actions: { ...state.actions, [action.key]: { pending: false, error: action.error } } };
      return action.toast ? withToast(failed, action.error) : failed;
    }
    case "error_reported":
      return withToast(state, action.error);
    case "toast_dismissed":
      return { ...state, toasts: state.toasts.filter((toast) => toast.id !== action.id) };
  }
}

function applyEvent(state: WorkbenchState, event: AppliedEvent): WorkbenchState {
  switch (event.type) {
    case "job":
      return withJob(state, event.job);
    case "progress": {
      const index = state.jobs.findIndex((job) => job.id === event.job_id);
      const job = state.jobs[index];
      if (job?.status !== "running") return state;
      const jobs = state.jobs.slice();
      jobs[index] = { ...job, progress: event.progress };
      return { ...state, jobs };
    }
    case "workspace": {
      if (!state.workspace) return state;
      const workspace = { ...state.workspace, ...event.sections };
      return { ...state, workspace, root: workspace.project.root };
    }
    case "file":
      return { ...state, fileRevisions: { ...state.fileRevisions, [event.path]: event.revision } };
  }
}

const RANK: Record<JobStatus, number> = { queued: 0, running: 1, succeeded: 2, failed: 2, cancelled: 2 };

/** Upserts by id and never moves a job backwards (queued < running < terminal). */
function withJob(state: WorkbenchState, incoming: JobSummary): WorkbenchState {
  const index = state.jobs.findIndex((job) => job.id === incoming.id);
  if (index === -1) return capJobs({ ...state, jobs: bySequence([incoming, ...state.jobs]) });
  const current = state.jobs[index]!;
  if (RANK[incoming.status] < RANK[current.status]) return state;
  const jobs = state.jobs.slice();
  jobs[index] = keepNewerProgress(current, incoming);
  return { ...state, jobs };
}

/** A job summary can be older than the progress events already applied. */
function keepNewerProgress(current: JobSummary, incoming: JobSummary): JobSummary {
  const bothRunning = current.status === "running" && incoming.status === "running";
  return bothRunning && isNewer(current.progress, incoming.progress) ? { ...incoming, progress: current.progress } : incoming;
}

function isNewer(progress: Progress | null, than: Progress | null): boolean {
  // RFC 3339 UTC timestamps with fixed precision order lexicographically.
  return progress !== null && (than === null || progress.updated_at > than.updated_at);
}

function bySequence(jobs: JobSummary[]): JobSummary[] {
  return jobs.sort((a, b) => b.sequence - a.sequence);
}

function capJobs(state: WorkbenchState): WorkbenchState {
  if (state.jobs.length <= MAX_JOBS) return state;
  const jobs = state.jobs.slice(0, MAX_JOBS);
  return { ...state, jobs, jobsNextBefore: String(jobs[jobs.length - 1]!.sequence) };
}

function withToast(state: WorkbenchState, error: ErrorBody): WorkbenchState {
  const toast = { id: state.nextToastId, code: error.code, message: error.message };
  return { ...state, toasts: [...state.toasts, toast].slice(-MAX_TOASTS), nextToastId: state.nextToastId + 1 };
}

function sameConnection(a: Connection, b: Connection): boolean {
  if (a.status === "disconnected" && b.status === "disconnected") return a.reason === b.reason;
  return a.status === b.status;
}
