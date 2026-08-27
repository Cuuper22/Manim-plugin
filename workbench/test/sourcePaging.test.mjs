import assert from "node:assert/strict";
import test from "node:test";
import { loadCompleteSource, MAX_SOURCE_BYTES, SOURCE_PAGE_LINES } from "../src/api/sourcePaging.ts";

function pageServer(lines, { revision = "rev-a", mutateRevisionAt = 0 } = {}) {
  const calls = [];
  const fetch = async (path, start, requestedEnd) => {
    calls.push([start, requestedEnd]);
    const end = Math.min(requestedEnd, lines.length);
    return {
      path,
      revision: calls.length === mutateRevisionAt ? "rev-b" : revision,
      language: "python",
      start_line: lines.length ? start : 1,
      end_line: lines.length ? end : 0,
      total_lines: lines.length,
      content: lines.length ? lines.slice(start - 1, end).join("\n") : "",
    };
  };
  return { calls, fetch };
}

test("assembles a 528-line source without dropping or duplicating the page boundary", async () => {
  const lines = Array.from({ length: 528 }, (_, index) => index === 399 || index === 400 ? "" : `line-${index + 1}`);
  const server = pageServer(lines);
  const source = await loadCompleteSource("scenes.py", server.fetch);

  assert.equal(SOURCE_PAGE_LINES, 400);
  assert.deepEqual(server.calls, [[1, 400], [401, 800]]);
  assert.equal(source.total_lines, 528);
  assert.equal(source.end_line, 528);
  assert.equal(source.complete, true);
  assert.equal(source.content, lines.join("\n"));
  assert.ok(source.bytes < MAX_SOURCE_BYTES);
});

test("rejects pages read from different source revisions", async () => {
  const server = pageServer(Array.from({ length: 528 }, (_, index) => `line-${index + 1}`), { mutateRevisionAt: 2 });
  await assert.rejects(loadCompleteSource("scenes.py", server.fetch), /revision changed between pages/);
});

test("handles an empty source as a complete zero-line document", async () => {
  const server = pageServer([]);
  const source = await loadCompleteSource("notes.txt", server.fetch);
  assert.equal(source.content, "");
  assert.equal(source.total_lines, 0);
  assert.equal(source.complete, true);
});
