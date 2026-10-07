import { EngineClient } from "../api/client.ts";
import { ApiError, asApiError } from "../api/errors.ts";
import { EventFeed, type OpenStream } from "../api/events.ts";
import type { JobSummary, OperationRequest } from "../api/types.ts";
import { initialState, reduce, type Connection, type StoreAction, type WorkbenchState } from "./reducer.ts";

/** How often a lost engine is tried again. */
export const RECONNECT_MS = 3000;

export type Outcome<T> = { ok: true; value: T } | { ok: false; error: ApiError };

/**
 * The one store the workbench reads: a snapshot from `GET /api/state` kept
 * current by the event stream, plus per-action pending/error state and
 * error toasts. Subscribe with `useSyncExternalStore`.
 */
export class WorkbenchStore {
  readonly #client: EngineClient;
  readonly #openStream: OpenStream;
  readonly #listeners = new Set<() => void>();
  #state: WorkbenchState = initialState;
  #feed: EventFeed | null = null;
  #reconnect: ReturnType<typeof setTimeout> | null = null;
  /** Bumped by `connect` and `disconnect`; late async results of older rounds are ignored. */
  #round = 0;

  constructor(client = new EngineClient(), openStream: OpenStream = (url) => new EventSource(url)) {
    this.#client = client;
    this.#openStream = openStream;
  }

  readonly getState = (): WorkbenchState => this.#state;

  readonly subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** Loads the workspace and follows its events, replacing any earlier connection. */
  async connect(): Promise<void> {
    const round = this.#stop();
    try {
      const snapshot = await this.#client.state();
      if (round !== this.#round) return;
      this.#dispatch({ type: "snapshot", state: snapshot });
      this.#feed = new EventFeed(snapshot.event_cursor, this.#openStream, () => this.#client.state(), {
        snapshot: (state) => this.#dispatch({ type: "snapshot", state }),
        event: (event) => this.#dispatch({ type: "event", event }),
        open: () => this.#dispatch({ type: "connection", connection: { status: "online" } }),
        dropped: (closed) => void this.#streamDropped(round, closed),
        failed: (error) => this.#lost(asApiError(error)),
      });
    } catch (error) {
      if (round === this.#round) this.#lost(asApiError(error));
    }
  }

  disconnect(): void {
    this.#stop();
  }

  /** Reloads the snapshot, e.g. after an artifact URL answered `410 artifact_changed`. */
  refresh(): void {
    if (this.#feed) this.#feed.resync();
    else void this.connect();
  }

  /**
   * Runs `task` as action `key`: pending while it runs, its error kept under
   * `actions[key]` and shown as a toast unless the caller handles its code
   * (`quiet`). A lost session or engine switches the connection state instead.
   */
  async perform<T>(
    key: string,
    task: (client: EngineClient) => Promise<T>,
    quiet: readonly string[] = [],
  ): Promise<Outcome<T>> {
    this.#dispatch({ type: "action_started", key });
    try {
      const value = await task(this.#client);
      this.#dispatch({ type: "action_succeeded", key });
      return { ok: true, value };
    } catch (caught) {
      const error = asApiError(caught);
      const lost = isConnectionLoss(error);
      this.#dispatch({ type: "action_failed", key, error: error.body, toast: !lost && !quiet.includes(error.code) });
      if (lost) this.#lost(error);
      return { ok: false, error };
    }
  }

  /** Submits an operation; the job is listed at once and then follows its events. */
  submit(key: string, request: OperationRequest): Promise<Outcome<JobSummary>> {
    return this.perform(key, async (client) => this.#tracked(await client.submitJob(request)));
  }

  cancel(jobId: string): Promise<Outcome<JobSummary>> {
    return this.perform(`cancel:${jobId}`, async (client) => this.#tracked(await client.cancelJob(jobId)));
  }

  async loadOlderJobs(): Promise<void> {
    const before = this.#state.jobsNextBefore;
    if (before === null) return;
    const outcome = await this.perform("jobs:older", (client) => client.jobs({ before }));
    if (outcome.ok) this.#dispatch({ type: "older_jobs", page: outcome.value });
  }

  dismissToast(id: number): void {
    this.#dispatch({ type: "toast_dismissed", id });
  }

  #tracked(job: JobSummary): JobSummary {
    this.#dispatch({ type: "job", job });
    return job;
  }

  #dispatch(action: StoreAction): void {
    const next = reduce(this.#state, action);
    if (next === this.#state) return;
    this.#state = next;
    for (const listener of this.#listeners) listener();
  }

  /** Ends the current round: no feed, no pending reconnect. Returns the new round. */
  #stop(): number {
    this.#feed?.close();
    this.#feed = null;
    if (this.#reconnect !== null) clearTimeout(this.#reconnect);
    this.#reconnect = null;
    return ++this.#round;
  }

  async #streamDropped(round: number, closed: boolean): Promise<void> {
    if (this.#state.connection.status === "online") {
      this.#dispatch({ type: "connection", connection: { status: "reconnecting" } });
    }
    try {
      await this.#client.health();
    } catch (error) {
      if (round === this.#round) this.#lost(asApiError(error));
      return;
    }
    // The engine is up. A stream the browser gave up on (e.g. 429 too_many_streams) starts over.
    if (closed && round === this.#round) this.#retryLater();
  }

  #lost(error: ApiError): void {
    this.#stop();
    const connection: Connection = error.code === "unauthorized"
      ? { status: "unauthorized" }
      : { status: "disconnected", reason: error.message };
    this.#dispatch({ type: "connection", connection });
    // A session comes back too: the engine's new link, opened in any tab of this browser, sets the cookie.
    this.#retryLater();
  }

  #retryLater(): void {
    if (this.#reconnect !== null) return;
    this.#reconnect = setTimeout(() => {
      this.#reconnect = null;
      void this.connect();
    }, RECONNECT_MS);
  }
}

function isConnectionLoss(error: ApiError): boolean {
  return error.code === "unauthorized" || error.code === "unreachable";
}
