import { ApiError } from "./errors.ts";
import type { SourceLanguage, SourcePage } from "./types.ts";

export const SOURCE_PAGE_LINES = 400;

/** A whole file read at one revision, byte-exact (CONTRACT-http §6.3 line model). */
export interface SourceDocument {
  path: string;
  revision: string;
  language: SourceLanguage;
  eol: "lf" | "crlf";
  final_newline: boolean;
  bytes: number;
  total_lines: number;
  content: string;
}

export type PageFetcher = (path: string, startLine: number, endLine: number) => Promise<SourcePage>;

function incomplete(path: string, reason: string): never {
  throw new ApiError("source_incomplete", `Source load incomplete: ${reason}.`, { path });
}

function lineCount(content: string): number {
  return content.split("\n").length;
}

/**
 * Reads `path` page by page from line 1. Fails unless every page comes from
 * the same revision and the pages reassemble into exactly the file's bytes.
 */
export async function loadCompleteSource(path: string, fetchPage: PageFetcher): Promise<SourceDocument> {
  if (!path) incomplete(path, "the path is empty");
  const first = await fetchPage(path, 1, SOURCE_PAGE_LINES);
  if (first.path !== path) incomplete(path, `the engine returned ${first.path}`);
  if (!first.revision) incomplete(path, "the page has no revision");
  const total = first.total_lines;
  if (!Number.isSafeInteger(total) || total < 0) incomplete(path, "the line count is invalid");
  if (total === 0 && (first.start_line !== 1 || first.end_line !== 0 || first.content !== "")) {
    incomplete(path, "the empty file's page is inconsistent");
  }

  const chunks: string[] = [];
  let page = first;
  let start = 1;
  let requestedEnd = SOURCE_PAGE_LINES;
  while (total > 0) {
    const sameFile = page.revision === first.revision
      && page.total_lines === total
      && page.bytes === first.bytes
      && page.final_newline === first.final_newline;
    if (!sameFile) incomplete(path, "the file changed between pages");
    // The engine may return fewer lines than asked (it caps pages); follow its end_line.
    const end = page.end_line;
    if (page.start_line !== start || end < start || end > Math.min(requestedEnd, total)) {
      incomplete(path, `expected a page starting at line ${start}, received lines ${page.start_line}-${end}`);
    }
    if (lineCount(page.content) !== end - start + 1) incomplete(path, `lines ${start}-${end} are truncated`);
    chunks.push(page.content);
    if (end === total) break;
    start = end + 1;
    requestedEnd = start + SOURCE_PAGE_LINES - 1;
    page = await fetchPage(path, start, requestedEnd);
  }

  const content = chunks.join("\n") + (first.final_newline ? "\n" : "");
  const bytes = new TextEncoder().encode(content).byteLength;
  if (bytes !== first.bytes) incomplete(path, `assembled ${bytes} bytes, the file has ${first.bytes}`);

  return {
    path,
    revision: first.revision,
    language: first.language,
    eol: first.eol,
    final_newline: first.final_newline,
    bytes,
    total_lines: total,
    content,
  };
}
