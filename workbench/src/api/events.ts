import type { ServerEvent, ServerEventType, WorkspaceState } from "./types.ts";

const EVENT_TYPES: readonly ServerEventType[] = ["job", "progress", "workspace", "file", "resync"];
/** `EventSource.CLOSED`: the browser stopped retrying (an HTTP error answered the stream). */
const CLOSED = 2;

/** The parts of `EventSource` the feed uses. */
export interface EventStream {
  readonly readyState: number;
  addEventListener(type: string, listener: (event: MessageEvent) => void): void;
  close(): void;
}

export type OpenStream = (url: string) => EventStream;

export type AppliedEvent = Exclude<ServerEvent, { type: "resync" }>;

export interface EventId {
  instance: string;
  seq: number;
}

/** `<instance_id>.<seq>`; `null` for anything else. */
export function parseEventId(id: string): EventId | null {
  const dot = id.lastIndexOf(".");
  const digits = id.slice(dot + 1);
  if (dot <= 0 || !/^\d+$/.test(digits)) return null;
  const seq = Number(digits);
  return Number.isSafeInteger(seq) ? { instance: id.slice(0, dot), seq } : null;
}

/**
 * Lets each event of one engine process through once, in order
 * (CONTRACT-http §8.5 step 3). Starts at a snapshot's `event_cursor`.
 */
export class EventGate {
  readonly instance: string;
  #applied: number;

  constructor(snapshotCursor: string) {
    const cursor = parseEventId(snapshotCursor);
    if (!cursor) throw new Error(`The engine sent a malformed event cursor: ${snapshotCursor}`);
    this.instance = cursor.instance;
    this.#applied = cursor.seq;
  }

  accept(id: string, type: ServerEventType): "apply" | "drop" | "resync" {
    const event = parseEventId(id);
    // Another process's ids cannot be ordered against ours: start over.
    if (!event || event.instance !== this.instance) return "resync";
    if (event.seq <= this.#applied) return "drop";
    this.#applied = event.seq;
    return type === "resync" ? "resync" : "apply";
  }
}

export interface FeedHandlers {
  /** A resync replaced everything applied so far with this snapshot. */
  snapshot(state: WorkspaceState): void;
  event(event: AppliedEvent): void;
  open(): void;
  /** The stream dropped. `closed`: the browser will not reconnect it by itself. */
  dropped(closed: boolean): void;
  /** A resync could not load the snapshot; the feed stops applying events. */
  failed(error: unknown): void;
}

/**
 * The SSE half of the store's data: resumes from a snapshot cursor, lets the
 * browser reconnect with `Last-Event-ID`, and answers `resync` by buffering,
 * reloading the snapshot and replaying what is newer (§8.5 step 5).
 */
export class EventFeed {
  readonly #stream: EventStream;
  readonly #loadSnapshot: () => Promise<WorkspaceState>;
  readonly #handlers: FeedHandlers;
  #gate: EventGate;
  /** Non-null while a snapshot loads: events wait here. */
  #buffer: [string, ServerEvent][] | null = null;
  #closed = false;

  constructor(
    snapshotCursor: string,
    open: OpenStream,
    loadSnapshot: () => Promise<WorkspaceState>,
    handlers: FeedHandlers,
  ) {
    this.#gate = new EventGate(snapshotCursor);
    this.#loadSnapshot = loadSnapshot;
    this.#handlers = handlers;
    this.#stream = open(`/api/events?after=${encodeURIComponent(snapshotCursor)}`);
    for (const type of EVENT_TYPES) this.#stream.addEventListener(type, (message) => this.#receive(message));
    this.#stream.addEventListener("open", () => {
      if (!this.#closed) handlers.open();
    });
    this.#stream.addEventListener("error", () => {
      if (!this.#closed) handlers.dropped(this.#stream.readyState === CLOSED);
    });
  }

  close(): void {
    this.#closed = true;
    this.#stream.close();
  }

  /** Reloads the snapshot without dropping the stream. */
  resync(): void {
    if (!this.#buffer) void this.#resync();
  }

  #receive(message: MessageEvent): void {
    if (this.#closed) return;
    const id = message.lastEventId;
    let event: ServerEvent;
    try {
      event = JSON.parse(String(message.data)) as ServerEvent;
    } catch {
      // Unreadable: whatever it said is recovered by a snapshot.
      event = { type: "resync", reason: "unknown_cursor" };
    }
    if (this.#buffer) {
      this.#buffer.push([id, event]);
    } else if (this.#pass(id, event)) {
      void this.#resync();
    }
  }

  /** Applies the event if the gate lets it through; `true` when a resync is needed. */
  #pass(id: string, event: ServerEvent): boolean {
    const decision = this.#gate.accept(id, event.type);
    if (decision === "apply" && event.type !== "resync") this.#handlers.event(event);
    return decision === "resync";
  }

  async #resync(): Promise<void> {
    for (;;) {
      this.#buffer = [];
      let snapshot: WorkspaceState;
      let gate: EventGate;
      try {
        snapshot = await this.#loadSnapshot();
        gate = new EventGate(snapshot.event_cursor);
      } catch (error) {
        this.#buffer = null;
        if (!this.#closed) this.#handlers.failed(error);
        return;
      }
      if (this.#closed) return;
      this.#gate = gate;
      this.#handlers.snapshot(snapshot);
      const buffered = this.#buffer;
      this.#buffer = null;
      let again = false;
      for (const [id, event] of buffered) {
        // Anything after a second resync was published before the next snapshot, so it is in it.
        if (this.#pass(id, event)) {
          again = true;
          break;
        }
      }
      if (!again) return;
    }
  }
}
