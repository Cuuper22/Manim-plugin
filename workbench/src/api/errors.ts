import type { ErrorBody } from "./types.ts";

/**
 * Codes the client produces itself; none of them collide with engine codes.
 * `unreachable`: nothing answered, or something other than the engine did.
 * `source_incomplete`: a paged source load did not add up to one revision.
 */
export type ClientErrorCode = "unreachable" | "source_incomplete";

/** Every failure the client reports. Branch on `code`, never on `message`. */
export class ApiError extends Error {
  readonly code: string;
  readonly data: Record<string, unknown> | null;
  /** The HTTP status, or 0 when no engine response was involved. */
  readonly status: number;

  constructor(code: string, message: string, data: Record<string, unknown> | null = null, status = 0) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.data = data;
    this.status = status;
  }

  get body(): ErrorBody {
    return { code: this.code, message: this.message, data: this.data };
  }
}

/** Wraps anything thrown as an `ApiError`; unexpected throws become `internal`. */
export function asApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error;
  const message = error instanceof Error && error.message ? error.message : String(error);
  return new ApiError("internal", message);
}

function isErrorBody(value: unknown): value is ErrorBody {
  if (typeof value !== "object" || value === null) return false;
  const body = value as Record<string, unknown>;
  return typeof body.code === "string"
    && typeof body.message === "string"
    && (body.data === null || body.data === undefined || typeof body.data === "object");
}

/**
 * The engine's error envelope as an `ApiError`. A failure response without
 * one came from something else, e.g. a dev proxy whose engine is down.
 */
export async function errorFromResponse(response: Response): Promise<ApiError> {
  const text = await response.text().catch(() => "");
  let envelope: unknown;
  try {
    envelope = JSON.parse(text);
  } catch {
    envelope = undefined;
  }
  const body = (envelope as { error?: unknown } | undefined)?.error;
  if (isErrorBody(body)) return new ApiError(body.code, body.message, body.data ?? null, response.status);
  return new ApiError(
    "unreachable",
    `Something other than the Manim Director engine answered (HTTP ${response.status}).`,
    { status: response.status },
    response.status,
  );
}

/** A `fetch` that rejected: refused connection, timeout or aborted. */
export function errorFromFailedFetch(error: unknown, timeoutSeconds: number): ApiError {
  const timedOut = error instanceof DOMException && error.name === "TimeoutError";
  return new ApiError(
    "unreachable",
    timedOut
      ? `The engine did not answer within ${timeoutSeconds} s.`
      : "Cannot reach the Manim Director engine.",
  );
}
