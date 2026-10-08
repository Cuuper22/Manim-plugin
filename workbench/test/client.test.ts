import assert from "node:assert/strict";
import test from "node:test";
import { EngineClient, type Fetch } from "../src/api/client.ts";
import { ApiError, asApiError } from "../src/api/errors.ts";
import { job, workspaceState } from "./fixtures.ts";

interface Sent {
  url: string;
  init: RequestInit;
}

function respond(answer: (sent: Sent) => Response | Promise<Response>): { client: EngineClient; sent: Sent[] } {
  const sent: Sent[] = [];
  const fetchImpl: Fetch = async (url, init) => {
    sent.push({ url, init });
    return answer({ url, init });
  };
  return { client: new EngineClient(fetchImpl), sent };
}

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

async function failure(promise: Promise<unknown>): Promise<ApiError> {
  try {
    await promise;
  } catch (error) {
    assert.ok(error instanceof ApiError, `expected an ApiError, got ${String(error)}`);
    return error;
  }
  assert.fail("expected the call to fail");
}

test("engine error envelopes become ApiErrors with code, data and status", async () => {
  const { client } = respond(() =>
    json({ error: { code: "queue_full", message: "The queue is full.", data: { capacity: 128 } } }, 429),
  );
  const error = await failure(client.submitJob({ operation: "doctor" }));
  assert.equal(error.code, "queue_full");
  assert.equal(error.message, "The queue is full.");
  assert.deepEqual(error.data, { capacity: 128 });
  assert.equal(error.status, 429);
});

test("401 keeps the unauthorized code the store signs out on", async () => {
  const { client } = respond(() => json({ error: { code: "unauthorized", message: "Open the link.", data: null } }, 401));
  const error = await failure(client.state());
  assert.equal(error.code, "unauthorized");
  assert.equal(error.data, null);
});

test("a failure without the engine's envelope means the engine was not reached", async () => {
  // e.g. the dev proxy answering for an engine that is not running
  const { client } = respond(() => new Response("", { status: 500 }));
  const error = await failure(client.health());
  assert.equal(error.code, "unreachable");
  assert.equal(error.status, 500);
  assert.match(error.message, /HTTP 500/);
});

test("a refused connection and a timeout are unreachable", async () => {
  const refused = respond(() => Promise.reject(new TypeError("fetch failed")));
  assert.equal((await failure(refused.client.state())).code, "unreachable");

  const slow = respond(() => Promise.reject(new DOMException("The operation timed out.", "TimeoutError")));
  const error = await failure(slow.client.health());
  assert.equal(error.code, "unreachable");
  assert.match(error.message, /within 3 s/);
});

test("a success that is not JSON, or another API version, is not this engine", async () => {
  const html = respond(() => new Response("<!doctype html>", { status: 200 }));
  assert.equal((await failure(html.client.state())).code, "unreachable");

  const v1 = respond(() => json({ ok: true, version: "1.1.0" }));
  const error = await failure(v1.client.health());
  assert.equal(error.code, "unreachable");
  assert.match(error.message, /not a Manim Director 2 engine/);
});

test("writes send JSON, bodiless ones send {}", async () => {
  const { client, sent } = respond(({ url }) => json(url.endsWith("/cancel") ? job({ status: "cancelled" }) : job()));
  await client.submitJob({ operation: "render", scene: "Recurrence", profile: "draft" });
  await client.cancelJob("a b");

  const [submit, cancel] = sent;
  assert.equal(submit?.url, "/api/jobs");
  assert.equal(submit?.init.method, "POST");
  assert.equal((submit?.init.headers as Record<string, string>)["Content-Type"], "application/json");
  assert.deepEqual(JSON.parse(String(submit?.init.body)), { operation: "render", scene: "Recurrence", profile: "draft" });
  assert.equal(cancel?.url, "/api/jobs/a%20b/cancel");
  assert.equal(cancel?.init.body, "{}");
  assert.equal(cancel?.init.credentials, "same-origin");
});

test("reads build their queries and send no body", async () => {
  const { client, sent } = respond(({ url }) =>
    url.startsWith("/api/state") ? json(workspaceState()) : json({ items: [], next_before: null, next_after: null }),
  );
  await client.state();
  await client.jobs({ before: "118", limit: 50 });
  await client.jobLogs("id", { after: "9" });
  await client.jobLogs("id");
  assert.deepEqual(sent.map((call) => call.url), [
    "/api/state",
    "/api/jobs?before=118&limit=50",
    "/api/jobs/id/logs?after=9",
    "/api/jobs/id/logs",
  ]);
  assert.ok(sent.every((call) => call.init.method === "GET" && call.init.body === undefined));
});

test("source pages are requested with the path encoded", async () => {
  const { client, sent } = respond(() =>
    json({
      path: "scenes/a b.py",
      revision: "r",
      language: "python",
      eol: "lf",
      final_newline: true,
      bytes: 2,
      total_lines: 1,
      start_line: 1,
      end_line: 1,
      content: "x",
    }),
  );
  const source = await client.loadSource("scenes/a b.py");
  assert.equal(source.content, "x\n");
  assert.equal(sent[0]?.url, "/api/source?path=scenes%2Fa+b.py&start_line=1&end_line=400");
});

test("anything thrown becomes an ApiError", () => {
  const known = new ApiError("not_found", "No such job.", { resource: "job", key: "x" }, 404);
  assert.equal(asApiError(known), known);
  const wrapped = asApiError(new Error("boom"));
  assert.equal(wrapped.code, "internal");
  assert.equal(wrapped.message, "boom");
});
