export const SOURCE_PAGE_LINES = 400;
export const MAX_SOURCE_BYTES = 2 * 1024 * 1024;

export interface SourcePage {
  path: string;
  revision: string;
  language: string;
  start_line: number;
  end_line: number;
  total_lines: number;
  content: string;
}

export interface CompleteSourceDocument extends SourcePage {
  start_line: 1;
  complete: true;
  bytes: number;
}

type PageFetcher = (path: string, startLine: number, endLine: number) => Promise<SourcePage>;

function fail(reason: string): never {
  throw new Error(`Source load incomplete: ${reason}`);
}

function pageLineCount(content: string, expected: number): number {
  if (expected === 0) return content.length === 0 ? 0 : content.split("\n").length;
  return content.length === 0 ? 1 : content.split("\n").length;
}

export function sourceByteLength(content: string): number {
  return new TextEncoder().encode(content).byteLength;
}

export async function loadCompleteSource(path: string, fetchPage: PageFetcher): Promise<CompleteSourceDocument> {
  if (!path) fail("source path is empty");
  const chunks: string[] = [];
  let nextStart = 1;
  let revision: string | undefined;
  let language = "text";
  let totalLines: number | undefined;

  while (totalLines === undefined || nextStart <= totalLines) {
    const requestedEnd = nextStart + SOURCE_PAGE_LINES - 1;
    const page = await fetchPage(path, nextStart, requestedEnd);
    if (!Number.isSafeInteger(page.total_lines) || page.total_lines < 0) fail("invalid total_lines");
    if (page.path !== path) fail(`server returned ${page.path} while loading ${path}`);
    if (!page.revision) fail("page has no revision");

    if (revision === undefined) {
      revision = page.revision;
      totalLines = page.total_lines;
      language = page.language;
    } else {
      if (page.revision !== revision) fail("source revision changed between pages");
      if (page.total_lines !== totalLines) fail("line count changed between pages");
    }

    if (totalLines === 0) {
      if (page.start_line !== 1 || page.end_line !== 0 || page.content !== "") fail("empty source page is inconsistent");
      break;
    }

    const expectedEnd = Math.min(requestedEnd, totalLines);
    if (page.start_line !== nextStart || page.end_line !== expectedEnd) {
      fail(`expected lines ${nextStart}-${expectedEnd}, received ${page.start_line}-${page.end_line}`);
    }
    const expectedLines = expectedEnd - nextStart + 1;
    if (pageLineCount(page.content, expectedLines) !== expectedLines) fail(`page ${nextStart}-${expectedEnd} is truncated`);
    chunks.push(page.content);
    nextStart = expectedEnd + 1;
  }

  if (revision === undefined || totalLines === undefined) fail("server returned no source pages");
  if (totalLines > 0 && nextStart !== totalLines + 1) fail("not every source line was loaded");

  // Pages omit their line terminators; one newline restores each page boundary.
  // The mutation endpoint preserves the existing terminal newline, so adding one
  // here would create an extra blank line for files that do not end in newline.
  const content = chunks.join("\n");
  const bytes = sourceByteLength(content);
  if (bytes > MAX_SOURCE_BYTES) fail(`assembled source is ${bytes} bytes; limit is ${MAX_SOURCE_BYTES}`);

  return {
    path,
    revision,
    language,
    start_line: 1,
    end_line: totalLines,
    total_lines: totalLines,
    content,
    complete: true,
    bytes,
  };
}
