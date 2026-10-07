import assert from "node:assert/strict";
import test from "node:test";
import { EngineClient } from "../src/api/client.ts";
import { RECONNECT_MS, WorkbenchStore } from "../src/store/store.ts";
import { cursor, FakeStream, job, settle, workspaceState } from "./fixtures.ts";

type Answer = () => Response | Promise<Response>;

const json = (body: unknown, status = 200) => () =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const envelope = (code: string, status: number) => json({ error: { code, message: `${code} happened`, data: null } }, status);
const refused: Answer = () => Promise.reject(new TypeError("fetch failed"));
const health = json({ ok: true, version: "2.0.0", api_version: 2, instance_id: "i" });

/** A store over a fake engine whose answer per route the test can change. */
function setup(routes: Record<string, Answer>) {
  const streams: FakeStream[] = [];
  const requested: string[] = [];
  const client = new EngineClient(async (url) => {
    requested.push(url);
    const route = Object.keys(routes).find((prefix) => url.startsWith(prefix));
    if (!route) throw new Error(`unexpected request ${url}`);
    return routes[route]!();
  });
  const store = new WorkbenchStore(client, (url) => {
    const stream = new FakeStream(url);
    streams.push(stream);
    return stream;
  });
  return { store, streams, requested };
}

test("connecting loads the snapshot, then follows events from its cursor", async () => {
  const { store, streams } = setup({ "/api/state": json(workspaceState({ event_cursor: cursor(7) })) });
  await store.connect();
  assert.equal(store.getState().connection.status, "online");
  assert.equal(streams[0]?.url, `/api/events?after=${encodeURIComponent(cursor(7))}`);

  streams[0]!.emit(cursor(8), { type: "job", job: job({ status: "running" }) });
  assert.equal(store.getState().jobs[0]?.status, "running");
  store.disconnect();
});

test("an unreachable engine shows why and is retried until it answers", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const routes: Record<string, Answer> = { "/api/state": refused };
  const { store } = setup(routes);
  await store.connect();
  assert.deepEqual(store.getState().connection, {
    status: "disconnected",
    reason: "Cannot reach the Manim Director engine.",
  });
  assert.deepEqual(store.getState().toasts, []);

  routes["/api/state"] = json(workspaceState());
  t.mock.timers.tick(RECONNECT_MS);
  await settle();
  assert.equal(store.getState().connection.status, "online");
  store.disconnect();
});

test("a rejected session signs out, and signs back in once the browser has a session again", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const routes: Record<string, Answer> = { "/api/state": envelope("unauthorized", 401) };
  const { store } = setup(routes);
  await store.connect();
  assert.equal(store.getState().connection.status, "unauthorized");
  assert.deepEqual(store.getState().toasts, []);

  t.mock.timers.tick(RECONNECT_MS);
  await settle();
  assert.equal(store.getState().connection.status, "unauthorized");

  // The engine's new link was opened in another tab: its cookie now comes along.
  routes["/api/state"] = json(workspaceState());
  t.mock.timers.tick(RECONNECT_MS);
  await settle();
  assert.equal(store.getState().connection.status, "online");
  store.disconnect();
});

test("actions are pending while they run; failures keep their error and toast unless quiet", async () => {
  const routes: Record<string, Answer> = { "/api/state": json(workspaceState()), "/api/jobs": json(job()) };
  const { store } = setup(routes);
  await store.connect();

  const submitted = store.submit("render", { operation: "render", scene: "Recurrence" });
  assert.deepEqual(store.getState().actions.render, { pending: true, error: null });
  const outcome = await submitted;
  assert.ok(outcome.ok);
  assert.equal(store.getState().actions.render, undefined);
  assert.equal(store.getState().jobs[0]?.id, job().id);

  routes["/api/jobs"] = envelope("queue_full", 429);
  const failed = await store.submit("render", { operation: "render" });
  assert.ok(!failed.ok && failed.error.code === "queue_full");
  assert.equal(store.getState().actions.render?.error?.code, "queue_full");
  assert.equal(store.getState().toasts.length, 1);

  routes["/api/source"] = envelope("revision_conflict", 409);
  await store.perform("save", (client) => client.writeSource({ path: "a.py", expected_revision: "r", edit: { kind: "replace_all", content: "" } }), ["revision_conflict"]);
  assert.equal(store.getState().actions.save?.error?.code, "revision_conflict");
  assert.equal(store.getState().toasts.length, 1);
  store.disconnect();
});

test("an action that finds the engine gone switches to the disconnected screen instead of a toast", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { store, streams } = setup({ "/api/state": json(workspaceState()), "/api/jobs": refused });
  await store.connect();
  await store.submit("doctor", { operation: "doctor" });
  assert.equal(store.getState().connection.status, "disconnected");
  assert.deepEqual(store.getState().toasts, []);
  assert.equal(streams[0]?.readyState, 2);
  store.disconnect();
});

test("a dropped stream reconnects quietly while the engine is up, and disconnects when it is not", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const routes: Record<string, Answer> = { "/api/state": json(workspaceState()), "/api/health": health };
  const { store, streams } = setup(routes);
  await store.connect();

  streams[0]!.fail(false);
  assert.equal(store.getState().connection.status, "reconnecting");
  await settle();
  streams[0]!.open();
  assert.equal(store.getState().connection.status, "online");

  // The browser gave up on the stream (e.g. too many streams): start over with a new snapshot.
  streams[0]!.fail(true);
  await settle();
  t.mock.timers.tick(RECONNECT_MS);
  await settle();
  assert.equal(streams.length, 2);
  assert.equal(store.getState().snapshots, 2);

  routes["/api/health"] = refused;
  streams[1]!.fail(false);
  await settle();
  assert.equal(store.getState().connection.status, "disconnected");
  store.disconnect();
});
