import { ApiError, errorFromFailedFetch, errorFromResponse } from "./errors.ts";
import { loadCompleteSource, type SourceDocument } from "./sourcePaging.ts";
import type {
  Health,
  Job,
  JobPage,
  JobSummary,
  LogPage,
  OperationRequest,
  SourcePage,
  SourceWrite,
  SourceWriteResult,
  WorkspaceState,
} from "./types.ts";

export const API_VERSION = 2;

const TIMEOUT_SECONDS = 15;
const HEALTH_TIMEOUT_SECONDS = 3;
const SOURCE_WRITE_TIMEOUT_SECONDS = 30;

export type Fetch = (url: string, init: RequestInit) => Promise<Response>;

interface Call {
  method?: "GET" | "POST" | "PUT";
  /** POST and PUT always send JSON; bodiless writes send `{}`. */
  body?: unknown;
  timeoutSeconds?: number;
}

/**
 * The engine's HTTP API v2. Same-origin only: the browser authenticates with
 * the session cookie that the engine's `?token=` link set.
 */
export class EngineClient {
  readonly #fetch: Fetch;

  constructor(fetchImpl: Fetch = (url, init) => fetch(url, init)) {
    this.#fetch = fetchImpl;
  }

  async health(): Promise<Health> {
    const health = await this.#call<Health>("/api/health", { timeoutSeconds: HEALTH_TIMEOUT_SECONDS });
    requireApiVersion(health.api_version);
    return health;
  }

  async state(): Promise<WorkspaceState> {
    const state = await this.#call<WorkspaceState>("/api/state");
    requireApiVersion(state.engine?.api_version);
    return state;
  }

  /** `202` for a new job, `200` for a cache hit or a coalesced duplicate; both return the job. */
  submitJob(request: OperationRequest): Promise<JobSummary> {
    return this.#call("/api/jobs", { method: "POST", body: request });
  }

  jobs(page: { before?: string; limit?: number } = {}): Promise<JobPage> {
    return this.#call(`/api/jobs${query({ before: page.before, limit: page.limit })}`);
  }

  job(id: string): Promise<Job> {
    return this.#call(`/api/jobs/${encodeURIComponent(id)}`);
  }

  /** Idempotent; a finished job comes back unchanged. */
  cancelJob(id: string): Promise<JobSummary> {
    return this.#call(`/api/jobs/${encodeURIComponent(id)}/cancel`, { method: "POST" });
  }

  jobLogs(id: string, page: { after?: string; limit?: number } = {}): Promise<LogPage> {
    return this.#call(`/api/jobs/${encodeURIComponent(id)}/logs${query({ after: page.after, limit: page.limit })}`);
  }

  sourcePage(path: string, startLine: number, endLine: number): Promise<SourcePage> {
    return this.#call(`/api/source${query({ path, start_line: startLine, end_line: endLine })}`);
  }

  /** The whole file at one revision; a save sends that revision back. */
  loadSource(path: string): Promise<SourceDocument> {
    return loadCompleteSource(path, (page, start, end) => this.sourcePage(page, start, end));
  }

  writeSource(write: SourceWrite): Promise<SourceWriteResult> {
    return this.#call("/api/source", { method: "PUT", body: write, timeoutSeconds: SOURCE_WRITE_TIMEOUT_SECONDS });
  }

  async #call<T>(url: string, call: Call = {}): Promise<T> {
    const method = call.method ?? "GET";
    const timeoutSeconds = call.timeoutSeconds ?? TIMEOUT_SECONDS;
    const headers: Record<string, string> = { Accept: "application/json" };
    const init: RequestInit = {
      method,
      headers,
      credentials: "same-origin",
      signal: AbortSignal.timeout(timeoutSeconds * 1000),
    };
    if (method !== "GET") {
      headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(call.body ?? {});
    }

    let response: Response;
    try {
      response = await this.#fetch(url, init);
    } catch (error) {
      throw errorFromFailedFetch(error, timeoutSeconds);
    }
    if (!response.ok) throw await errorFromResponse(response);
    try {
      return (await response.json()) as T;
    } catch {
      throw new ApiError("unreachable", "The server on this port did not answer like the engine (no JSON).", null, response.status);
    }
  }
}

function query(params: Record<string, string | number | undefined>): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined) search.set(key, String(value));
  }
  const encoded = search.toString();
  return encoded ? `?${encoded}` : "";
}

function requireApiVersion(version: number | undefined): void {
  if (version !== API_VERSION) {
    throw new ApiError(
      "unreachable",
      `The server on this port is not a Manim Director ${API_VERSION} engine (API version ${version ?? "unknown"}).`,
      { api_version: version ?? null },
    );
  }
}
