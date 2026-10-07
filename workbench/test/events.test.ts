import assert from "node:assert/strict";
import test from "node:test";
import { EventFeed, EventGate, parseEventId, type AppliedEvent } from "../src/api/events.ts";
import type { ServerEvent, WorkspaceState } from "../src/api/types.ts";
import { cursor, deferred, FakeStream, INSTANCE, job, settle, workspaceState } from "./fixtures.ts";

const fileEvent = (revision: string): ServerEvent => ({ type: "file", path: "scenes/main.py", revision });

test("event ids parse as <instance>.<seq> and nothing else", () => {
  assert.deepEqual(parseEventId(cursor(42)), { instance: INSTANCE, seq: 42 });
  for (const bad of ["", "42", ".1", `${INSTANCE}.`, `${INSTANCE}.-1`, `${INSTANCE}.1e3`, `${INSTANCE}.99999999999999999999`]) {
    assert.equal(parseEventId(bad), null, bad);
  }
});

test("the gate applies each newer event once and asks for a resync on foreign or resync ids", () => {
  const gate = new EventGate(cursor(5));
  assert.equal(gate.accept(cursor(5), "job"), "drop");
  assert.equal(gate.accept(cursor(6), "job"), "apply");
  assert.equal(gate.accept(cursor(6), "job"), "drop");
  assert.equal(gate.accept(cursor(4), "file"), "drop");
  assert.equal(gate.accept(cursor(9), "resync"), "resync");
  assert.equal(gate.accept(cursor(10, "another-engine"), "job"), "resync");
  assert.equal(gate.accept("garbage", "job"), "resync");
  assert.throws(() => new EventGate("garbage"), /malformed event cursor/);
});

interface Harness {
  stream: FakeStream;
  applied: AppliedEvent[];
  snapshots: WorkspaceState[];
  calls: string[];
  feed: EventFeed;
}

function harness(loadSnapshot: () => Promise<WorkspaceState>, start = cursor(10)): Harness {
  const streams: FakeStream[] = [];
  const applied: AppliedEvent[] = [];
  const snapshots: WorkspaceState[] = [];
  const calls: string[] = [];
  const feed = new EventFeed(
    start,
    (url) => {
      const stream = new FakeStream(url);
      streams.push(stream);
      return stream;
    },
    loadSnapshot,
    {
      snapshot: (state) => snapshots.push(state),
      event: (event) => applied.push(event),
      open: () => calls.push("open"),
      dropped: (closed) => calls.push(closed ? "closed" : "dropped"),
      failed: (error) => calls.push(`failed: ${String(error)}`),
    },
  );
  return { stream: streams[0]!, applied, snapshots, calls, feed };
}

const never = () => new Promise<WorkspaceState>(() => {});

test("the stream resumes after the snapshot cursor and drops replays of applied events", () => {
  const { stream, applied } = harness(never);
  assert.equal(stream.url, `/api/events?after=${encodeURIComponent(cursor(10))}`);
  stream.emit(cursor(10), fileEvent("old"));
  stream.emit(cursor(11), fileEvent("r11"));
  // The browser reconnects with Last-Event-ID; the engine may replay what it already sent.
  stream.emit(cursor(11), fileEvent("r11"));
  stream.emit(cursor(12), { type: "job", job: job() });
  assert.deepEqual(applied.map((event) => event.type), ["file", "job"]);
});

test("a resync buffers events, reloads the snapshot and replays only what is newer", async () => {
  const snapshot = deferred<WorkspaceState>();
  const { stream, applied, snapshots } = harness(() => snapshot.promise);
  stream.emit(cursor(20), { type: "resync", reason: "lagged" });
  stream.emit(cursor(21), fileEvent("r21"));
  stream.emit(cursor(22), fileEvent("r22"));
  assert.deepEqual(applied, []);

  snapshot.resolve(workspaceState({ event_cursor: cursor(21) }));
  await settle();
  assert.equal(snapshots.length, 1);
  assert.deepEqual(applied, [fileEvent("r22")]);

  stream.emit(cursor(23), fileEvent("r23"));
  assert.deepEqual(applied, [fileEvent("r22"), fileEvent("r23")]);
});

test("an engine restart behind the stream (foreign instance) resyncs onto the new instance", async () => {
  const restarted = "a1b2c3d4-0000-4000-8000-000000000000";
  const { stream, applied, snapshots } = harness(async () => workspaceState({ event_cursor: cursor(3, restarted) }));
  stream.emit(cursor(3, restarted), { type: "resync", reason: "unknown_cursor" });
  await settle();
  assert.equal(snapshots.length, 1);
  stream.emit(cursor(4, restarted), fileEvent("r4"));
  assert.deepEqual(applied, [fileEvent("r4")]);
});

test("a second resync while buffering loads another snapshot and discards what it covers", async () => {
  const loads = [deferred<WorkspaceState>(), deferred<WorkspaceState>()];
  let call = 0;
  const { stream, applied, snapshots } = harness(() => loads[call++]!.promise);
  stream.emit(cursor(20), { type: "resync", reason: "expired" });
  stream.emit(cursor(21), fileEvent("r21"));
  stream.emit(cursor(30), { type: "resync", reason: "lagged" });
  stream.emit(cursor(31), fileEvent("covered"));

  loads[0]!.resolve(workspaceState({ event_cursor: cursor(20) }));
  await settle();
  assert.equal(call, 2);
  assert.deepEqual(applied, [fileEvent("r21")]);

  stream.emit(cursor(32), fileEvent("r32"));
  loads[1]!.resolve(workspaceState({ event_cursor: cursor(31) }));
  await settle();
  assert.equal(snapshots.length, 2);
  assert.deepEqual(applied, [fileEvent("r21"), fileEvent("r32")]);
});

test("a failed resync is reported and stops applying events", async () => {
  const { stream, applied, calls } = harness(async () => {
    throw new Error("down");
  });
  stream.emit(cursor(11), { type: "resync", reason: "lagged" });
  await settle();
  assert.deepEqual(calls, ["failed: Error: down"]);
  assert.deepEqual(applied, []);
});

test("an unreadable event is treated as a resync", async () => {
  let loads = 0;
  const { stream } = harness(async () => {
    loads += 1;
    return workspaceState({ event_cursor: cursor(11) });
  });
  stream.emitRaw("job", cursor(11), "{not json");
  await settle();
  assert.equal(loads, 1);
});

test("stream drops are reported with whether the browser gave up, and nothing after close", () => {
  const { stream, calls, feed } = harness(never);
  stream.open();
  stream.fail(false);
  stream.fail(true);
  feed.close();
  stream.fail(true);
  stream.emit(cursor(11), fileEvent("late"));
  assert.deepEqual(calls, ["open", "dropped", "closed"]);
});
