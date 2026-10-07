import assert from "node:assert/strict";
import test from "node:test";
import { ApiError } from "../src/api/errors.ts";
import { loadCompleteSource, SOURCE_PAGE_LINES, type PageFetcher } from "../src/api/sourcePaging.ts";
import type { SourcePage } from "../src/api/types.ts";

/** Pages a file exactly like the engine's line model: split on "\n", the piece after a final "\n" is no line. */
function pageServer(text: string, options: { revisionAt?: (call: number) => string; pageCap?: number } = {}) {
  const lines = text === "" ? [] : text.split("\n");
  const finalNewline = text.endsWith("\n");
  if (finalNewline) lines.pop();
  const calls: [number, number][] = [];
  const fetch: PageFetcher = async (path, start, requestedEnd) => {
    calls.push([start, requestedEnd]);
    const end = Math.min(requestedEnd, start + (options.pageCap ?? Infinity) - 1, lines.length);
    const page: SourcePage = {
      path,
      revision: options.revisionAt?.(calls.length) ?? "rev-a",
      language: "python",
      eol: text.includes("\r\n") ? "crlf" : "lf",
      final_newline: finalNewline,
      bytes: new TextEncoder().encode(text).byteLength,
      total_lines: lines.length,
      start_line: start,
      end_line: lines.length ? end : 0,
      content: lines.slice(start - 1, end).join("\n"),
    };
    return page;
  };
  return { calls, fetch };
}

test("assembles a 528-line file across the page boundary without dropping or duplicating lines", async () => {
  const lines = Array.from({ length: 528 }, (_, index) => (index === 399 || index === 400 ? "" : `line-${index + 1}`));
  const text = `${lines.join("\n")}\n`;
  const server = pageServer(text);
  const source = await loadCompleteSource("scenes/main.py", server.fetch);

  assert.equal(SOURCE_PAGE_LINES, 400);
  assert.deepEqual(server.calls, [[1, 400], [401, 800]]);
  assert.equal(source.total_lines, 528);
  assert.equal(source.content, text);
  assert.equal(source.revision, "rev-a");
});

test("round-trips CRLF, trailing blank lines and a missing final newline byte for byte", async () => {
  for (const text of ["a\r\nb\r\n\r\n", "x", "a\n\n", "\n", "é ∑\n"]) {
    const source = await loadCompleteSource("notes.md", pageServer(text).fetch);
    assert.equal(source.content, text, JSON.stringify(text));
    assert.equal(source.bytes, new TextEncoder().encode(text).byteLength);
  }
  const crlf = await loadCompleteSource("notes.md", pageServer("a\r\nb\r\n").fetch);
  assert.equal(crlf.eol, "crlf");
});

test("follows a page the engine cut shorter than asked", async () => {
  const text = Array.from({ length: 30 }, (_, index) => `l${index}`).join("\n");
  const server = pageServer(text, { pageCap: 12 });
  const source = await loadCompleteSource("a.py", server.fetch);
  assert.deepEqual(server.calls.map(([start]) => start), [1, 13, 25]);
  assert.equal(source.content, text);
});

test("rejects pages read from different revisions", async () => {
  const text = Array.from({ length: 528 }, (_, index) => `line-${index + 1}`).join("\n");
  const server = pageServer(text, { revisionAt: (call) => (call === 2 ? "rev-b" : "rev-a") });
  await assert.rejects(loadCompleteSource("scenes/main.py", server.fetch), (error: unknown) => {
    assert.ok(error instanceof ApiError);
    assert.equal(error.code, "source_incomplete");
    assert.match(error.message, /changed between pages/);
    return true;
  });
});

test("rejects a page whose content is missing lines", async () => {
  const server = pageServer("a\nb\nc\n");
  const truncated: PageFetcher = async (...args) => ({ ...(await server.fetch(...args)), content: "a\nb" });
  await assert.rejects(loadCompleteSource("a.py", truncated), /truncated/);
});

test("rejects reassembled content whose size differs from the file", async () => {
  const server = pageServer("a\nb\n");
  const wrongSize: PageFetcher = async (...args) => ({ ...(await server.fetch(...args)), bytes: 99 });
  await assert.rejects(loadCompleteSource("a.py", wrongSize), /assembled 4 bytes, the file has 99/);
});

test("loads an empty file as zero lines", async () => {
  const source = await loadCompleteSource("notes.txt", pageServer("").fetch);
  assert.equal(source.content, "");
  assert.equal(source.total_lines, 0);
  assert.equal(source.final_newline, false);
});
