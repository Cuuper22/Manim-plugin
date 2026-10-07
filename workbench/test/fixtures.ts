import type { EventStream } from "../src/api/events.ts";
import type { JobSummary, Progress, ServerEvent, WorkspaceState } from "../src/api/types.ts";

export const INSTANCE = "0b6f2c1e-6a0f-4c2e-9a51-1f7c3d0d2a11";

export function cursor(seq: number, instance = INSTANCE): string {
  return `${instance}.${seq}`;
}

export function job(overrides: Partial<JobSummary> = {}): JobSummary {
  return {
    id: "7f1c2b0e-0d5e-4b1a-9a57-3c1f7d0c2a11",
    sequence: 1,
    operation: "render",
    status: "queued",
    origin: "http",
    cached: false,
    cached_from: null,
    source_job_id: null,
    cancel_requested: false,
    request: { operation: "render", scene: "Recurrence", file: "scenes/main.py", profile: "draft" },
    scene_id: "scenes/main.py#Recurrence",
    profile: "draft",
    created_at: "2026-10-07T10:31:02.114Z",
    started_at: null,
    finished_at: null,
    progress: null,
    error: null,
    artifacts: [],
    artifacts_total: 0,
    ...overrides,
  };
}

export function progress(current: number, updatedAt: string): Progress {
  return { phase: "animate", current, total: 10, scene_seconds: null, message: null, updated_at: updatedAt };
}

export function workspaceState(overrides: Partial<WorkspaceState> = {}): WorkspaceState {
  return {
    engine: {
      version: "2.0.0",
      api_version: 2,
      instance_id: INSTANCE,
      started_at: "2026-10-07T10:00:00.000Z",
      limits: { request_body_bytes: 262144, source_body_bytes: 3145728, source_file_bytes: 2097152, source_page_lines: 2000 },
    },
    event_cursor: cursor(0),
    jobs: [],
    jobs_next_before: null,
    project: {
      root: "/home/me/fibonacci",
      name: "Generalized Fibonacci",
      description: null,
      spec_file: "director.yaml",
      source_dir: "scenes",
      asset_dir: "assets",
      output_dir: "output",
      media_dir: "media",
      theme: "midnight",
      default_profile: "draft",
      duration_seconds: null,
      counts: { sources: 1, assets: 0, outputs: 0 },
    },
    spec: { path: "director.yaml", valid: true, revision: "abc", error: null },
    profiles: [],
    themes: [],
    scene_index: { state: "ready", indexed_at: "2026-10-07T10:00:01.000Z", files: 1, truncated: false, error: null },
    scenes: [],
    storyboard: [],
    latest: {},
    findings: [],
    doctor: null,
    ...overrides,
  };
}

/** An `EventSource` stand-in that tests drive by hand. */
export class FakeStream implements EventStream {
  readyState = 0;
  readonly url: string;
  readonly #listeners = new Map<string, ((event: MessageEvent) => void)[]>();

  constructor(url: string) {
    this.url = url;
  }

  addEventListener(type: string, listener: (event: MessageEvent) => void): void {
    this.#listeners.set(type, [...(this.#listeners.get(type) ?? []), listener]);
  }

  close(): void {
    this.readyState = 2;
  }

  open(): void {
    this.readyState = 1;
    this.#fire("open", new MessageEvent("open"));
  }

  fail(closed: boolean): void {
    this.readyState = closed ? 2 : 0;
    this.#fire("error", new MessageEvent("error"));
  }

  emit(id: string, event: ServerEvent): void {
    this.emitRaw(event.type, id, JSON.stringify(event));
  }

  emitRaw(type: string, id: string, data: string): void {
    this.#fire(type, new MessageEvent(type, { data, lastEventId: id }));
  }

  #fire(type: string, event: MessageEvent): void {
    for (const listener of this.#listeners.get(type) ?? []) listener(event);
  }
}

/** A promise with its resolver, for snapshots that arrive when a test says so. */
export function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/** Lets pending promise callbacks run. */
export function settle(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve));
}
