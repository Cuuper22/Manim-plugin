import assert from "node:assert/strict";
import test from "node:test";
import type { DoctorSnapshot } from "../src/api/types.ts";
import { initialState, MAX_JOBS, reduce, type StoreAction, type WorkbenchState } from "../src/store/reducer.ts";
import { job, progress, workspaceState } from "./fixtures.ts";

function run(...actions: StoreAction[]): WorkbenchState {
  return actions.reduce(reduce, initialState);
}

const snapshot = (jobs = [job()]): StoreAction => ({ type: "snapshot", state: workspaceState({ jobs, jobs_next_before: "7" }) });

test("a snapshot fills the workspace, the jobs and the project root, and counts itself", () => {
  const state = run(snapshot());
  assert.equal(state.connection.status, "online");
  assert.equal(state.root, "/home/me/fibonacci");
  assert.equal(state.workspace?.project.name, "Generalized Fibonacci");
  assert.equal(state.workspace?.engine.api_version, 2);
  assert.equal(state.jobs.length, 1);
  assert.equal(state.jobsNextBefore, "7");
  assert.equal(state.snapshots, 1);
  assert.ok(!("event_cursor" in state.workspace!));
});

test("job events upsert by id and never move a job backwards", () => {
  const queued = job({ status: "queued" });
  const running = job({ status: "running", started_at: "2026-10-07T10:31:03.000Z" });
  const done = job({ status: "succeeded", finished_at: "2026-10-07T10:31:09.000Z" });

  let state = run(snapshot([]), { type: "event", event: { type: "job", job: queued } });
  state = reduce(state, { type: "event", event: { type: "job", job: running } });
  assert.equal(state.jobs[0]?.status, "running");
  state = reduce(state, { type: "event", event: { type: "job", job: done } });
  // A late 202 response or a replayed event carries an older status.
  for (const stale of [queued, running]) {
    assert.equal(reduce(state, { type: "job", job: stale }), state);
  }
  assert.equal(state.jobs.length, 1);
  assert.equal(state.jobs[0]?.status, "succeeded");
});

test("new jobs are ordered newest first by sequence", () => {
  const state = run(
    snapshot([job({ id: "b", sequence: 5 })]),
    { type: "job", job: job({ id: "c", sequence: 9 }) },
    { type: "job", job: job({ id: "a", sequence: 2 }) },
  );
  assert.deepEqual(state.jobs.map((item) => item.id), ["c", "b", "a"]);
});

test("progress applies to running jobs only, and a stale summary keeps the newer progress", () => {
  const running = job({ status: "running" });
  let state = run(snapshot([running, job({ id: "q", sequence: 0 })]));
  const late = progress(7, "2026-10-07T10:31:05.000Z");
  state = reduce(state, { type: "event", event: { type: "progress", job_id: running.id, progress: late } });
  assert.deepEqual(state.jobs[0]?.progress, late);

  const ignored = reduce(state, { type: "event", event: { type: "progress", job_id: "q", progress: late } });
  assert.equal(ignored, state);

  const coalesced = job({ status: "running", progress: progress(3, "2026-10-07T10:31:04.000Z") });
  state = reduce(state, { type: "job", job: coalesced });
  assert.deepEqual(state.jobs[0]?.progress, late);
});

test("workspace events replace whole sections, including a section that became null", () => {
  const doctor = { job_id: "d", finished_at: null } as unknown as DoctorSnapshot;
  let state = run({ type: "snapshot", state: workspaceState({ doctor }) });
  state = reduce(state, {
    type: "event",
    event: { type: "workspace", sections: { doctor: null, scene_index: { state: "indexing", indexed_at: null, files: 0, truncated: false, error: null } } },
  });
  assert.equal(state.workspace?.doctor, null);
  assert.equal(state.workspace?.scene_index.state, "indexing");
  assert.equal(state.workspace?.project.name, "Generalized Fibonacci");
});

test("workspace events before the first snapshot are ignored", () => {
  const state = run({ type: "event", event: { type: "workspace", sections: { doctor: null } } });
  assert.equal(state, initialState);
});

test("file events record the newest revision and deletions", () => {
  const state = run(
    snapshot(),
    { type: "event", event: { type: "file", path: "scenes/main.py", revision: "r2" } },
    { type: "event", event: { type: "file", path: "scenes/old.py", revision: null } },
  );
  assert.deepEqual(state.fileRevisions, { "scenes/main.py": "r2", "scenes/old.py": null });
});

test("older pages append behind the newest jobs, skipping ones already listed", () => {
  const state = run(snapshot([job({ id: "n", sequence: 9 })]), {
    type: "older_jobs",
    page: { items: [job({ id: "n", sequence: 9 }), job({ id: "o", sequence: 3 })], next_before: null },
  });
  assert.deepEqual(state.jobs.map((item) => item.id), ["n", "o"]);
  assert.equal(state.jobsNextBefore, null);
});

test("the job list keeps the newest and pages from the oldest kept", () => {
  const many = Array.from({ length: MAX_JOBS }, (_, index) => job({ id: `j${index}`, sequence: index + 1 }));
  const state = run(snapshot(many), { type: "job", job: job({ id: "new", sequence: MAX_JOBS + 1 }) });
  assert.equal(state.jobs.length, MAX_JOBS);
  assert.equal(state.jobs[0]?.id, "new");
  assert.equal(state.jobsNextBefore, "2");
});

test("actions track pending and errors, and failures toast unless told not to", () => {
  const error = { code: "queue_full", message: "The queue is full.", data: { capacity: 128 } };
  let state = run({ type: "action_started", key: "render" });
  assert.deepEqual(state.actions.render, { pending: true, error: null });

  state = reduce(state, { type: "action_failed", key: "render", error, toast: true });
  assert.deepEqual(state.actions.render, { pending: false, error });
  assert.deepEqual(state.toasts, [{ id: 1, code: "queue_full", message: "The queue is full." }]);

  state = reduce(state, { type: "action_failed", key: "save", error, toast: false });
  assert.equal(state.toasts.length, 1);

  state = reduce(state, { type: "action_succeeded", key: "render" });
  assert.equal(state.actions.render, undefined);
  state = reduce(state, { type: "toast_dismissed", id: 1 });
  assert.deepEqual(state.toasts, []);
});

test("an unchanged connection status keeps the state object", () => {
  const state = run({ type: "connection", connection: { status: "disconnected", reason: "down" } });
  assert.equal(reduce(state, { type: "connection", connection: { status: "disconnected", reason: "down" } }), state);
  assert.notEqual(reduce(state, { type: "connection", connection: { status: "disconnected", reason: "other" } }), state);
});
