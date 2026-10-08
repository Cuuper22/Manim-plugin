import assert from "node:assert/strict";
import test from "node:test";
import { EditorState, Text, type TransactionSpec } from "@codemirror/state";
import { ApiError } from "../src/api/errors.ts";
import type { SourceDocument } from "../src/api/sourcePaging.ts";
import type { SourceWrite } from "../src/api/types.ts";
import { EditorSession, minimalChange, type EditorPort, type SourceAccess } from "../src/editor/session.ts";
import type { Outcome } from "../src/store/store.ts";
import { settle } from "./fixtures.ts";

/** A fake engine holding files as `[revision, content]`; revisions count up per write. */
function engine(files: Record<string, string>) {
  const disk = new Map(Object.entries(files).map(([path, content]) => [path, { revision: "r1", content }]));
  const writes: SourceWrite[] = [];
  let next = 2;
  const fail = <T>(code: string, data: Record<string, unknown> | null = null): Outcome<T> => ({
    ok: false,
    error: new ApiError(code, `${code} happened`, data),
  });
  const access: SourceAccess = {
    async load(path) {
      const file = disk.get(path);
      if (!file) return fail("not_found");
      const document: SourceDocument = {
        path,
        revision: file.revision,
        language: "python",
        eol: file.content.includes("\r\n") ? "crlf" : "lf",
        final_newline: file.content.endsWith("\n"),
        bytes: file.content.length,
        total_lines: 0,
        content: file.content,
      };
      return { ok: true, value: document };
    },
    async write(write) {
      writes.push(write);
      const current = disk.get(write.path)?.revision ?? null;
      if (current !== write.expected_revision) return fail("revision_conflict", { current_revision: current });
      if (write.edit.kind !== "replace_all") throw new Error("only replace_all");
      if (write.edit.content.includes("syntax error")) return fail("source_invalid", { line: 2 });
      const revision = `r${next++}`;
      disk.set(write.path, { revision, content: write.edit.content });
      return {
        ok: true,
        value: { path: write.path, previous_revision: current, revision, bytes: 0, total_lines: 0, affected_scenes: [] },
      };
    },
    async revision(path) {
      return { ok: true, value: disk.get(path)?.revision ?? null };
    },
  };
  /** An edit made by someone else, e.g. an agent. */
  const external = (path: string, content: string | null) => {
    if (content === null) disk.delete(path);
    else disk.set(path, { revision: `r${next++}`, content });
    return disk.get(path)?.revision ?? null;
  };
  return { access, disk, writes, external };
}

/** A view stand-in: applies transactions to its state and reports them like an update listener. */
function port(session: EditorSession): EditorPort & { type(text: string): void } {
  let state = EditorState.create();
  const view = {
    get state() {
      return state;
    },
    setState(next: EditorState) {
      state = next;
    },
    dispatch(spec: TransactionSpec) {
      state = state.update(spec).state;
      session.viewChanged(state);
    },
    reveal() {},
    type(text: string) {
      view.dispatch({ changes: { from: state.doc.length, insert: text } });
    },
  };
  return view;
}

test("a CRLF file round-trips byte-exactly and saves with the revision it was loaded at", async () => {
  const { access, writes, disk } = engine({ "a.py": "x = 1\r\ny = 2\r\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", 1);
  view.type("z = 3\r\n");
  assert.equal(session.getSnapshot().docs[0]?.dirty, true);

  assert.equal(await session.save(), true);
  assert.deepEqual(writes[0], {
    path: "a.py",
    expected_revision: "r1",
    edit: { kind: "replace_all", content: "x = 1\r\ny = 2\r\nz = 3\r\n" },
  });
  assert.equal(disk.get("a.py")?.content, "x = 1\r\ny = 2\r\nz = 3\r\n");
  assert.equal(session.getSnapshot().docs[0]?.dirty, false);
});

test("an external edit reloads a clean buffer and keeps the user's place", async () => {
  const { access, external } = engine({ "a.py": "one\ntwo\nthree\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", null);
  view.dispatch({ selection: { anchor: 9 } });

  session.diskChanged("a.py", external("a.py", "zero\none\ntwo\nthree\n"));
  await settle();
  assert.equal(view.state.sliceDoc(), "zero\none\ntwo\nthree\n");
  assert.equal(view.state.selection.main.head, 14, "the cursor stays on `three`");
  assert.equal(session.getSnapshot().docs[0]?.issue, null);
});

test("an external edit to a dirty buffer is a conflict: no silent save, then reload or a checked overwrite", async () => {
  const { access, writes, external, disk } = engine({ "a.py": "a\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", null);
  view.type("mine\n");

  session.diskChanged("a.py", external("a.py", "theirs\n"));
  assert.deepEqual(session.getSnapshot().docs[0]?.issue, { kind: "changed" });
  assert.equal(await session.save(), false);
  assert.equal(writes.length, 0);

  assert.equal(await session.overwrite(), true);
  assert.equal(writes[0]?.expected_revision, "r2", "checked against the revision read just before");
  assert.equal(disk.get("a.py")?.content, "a\nmine\n");

  view.type("again\n");
  session.diskChanged("a.py", external("a.py", "theirs again\n"));
  await session.reload();
  assert.equal(view.state.sliceDoc(), "theirs again\n");
  assert.equal(session.getSnapshot().docs[0]?.dirty, false);
});

test("a save that races an external write reports the conflict and keeps the buffer", async () => {
  const { access, external } = engine({ "a.py": "a\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", null);
  view.type("mine\n");
  external("a.py", "theirs\n");

  assert.equal(await session.save(), false);
  assert.deepEqual(session.getSnapshot().docs[0]?.issue, { kind: "changed" });
  assert.equal(view.state.sliceDoc(), "a\nmine\n");
});

test("a save is not mistaken for an external change, whether its file event comes late or never", async () => {
  const { access } = engine({ "notes.md": "a\n" });
  let loads = 0;
  const counted: SourceAccess = {
    ...access,
    load: (path, quiet) => {
      loads += 1;
      return access.load(path, quiet);
    },
  };
  const session = new EditorSession(counted, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("notes.md", null);
  view.type("b\n");
  const saving = session.save();
  view.type("c\n");
  assert.equal(await saving, true);
  assert.equal(session.getSnapshot().docs[0]?.issue, null, "typing on during a save is no conflict");
  assert.equal(session.getSnapshot().docs[0]?.dirty, true);
  assert.equal(loads, 1, "the file just written is not read back");

  session.diskChanged("notes.md", "r2");
  assert.equal(session.getSnapshot().docs[0]?.issue, null);
  assert.equal(await session.save(), true);
});

test("a conflict's revision is remembered, so recreating a file deleted without an event still saves cleanly", async () => {
  const { access, external, disk } = engine({ "notes.md": "a\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("notes.md", null);
  view.type("b\n");
  external("notes.md", null);
  assert.equal(await session.save(), false);
  assert.deepEqual(session.getSnapshot().docs[0]?.issue, { kind: "deleted" });

  const saving = session.save();
  view.type("c\n");
  assert.equal(await saving, true);
  assert.equal(disk.get("notes.md")?.content, "a\nb\n");
  assert.equal(session.getSnapshot().docs[0]?.issue, null);
});

test("invalid source stays unsaved with the engine's line; a deleted file is recreated only if still absent", async () => {
  const { access, writes, external, disk } = engine({ "a.py": "a\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", null);
  view.type("syntax error\n");
  assert.equal(await session.save(), false);
  assert.deepEqual(session.getSnapshot().docs[0]?.issue, { kind: "invalid", message: "source_invalid happened", line: 2 });

  session.diskChanged("a.py", external("a.py", null));
  assert.deepEqual(session.getSnapshot().docs[0]?.issue, { kind: "deleted" });
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "fixed\n" } });
  assert.equal(await session.save(), true);
  assert.equal(writes.at(-1)?.expected_revision, null);
  assert.equal(disk.get("a.py")?.content, "fixed\n");
});

test("a file already saved is not written again, but a checked overwrite of a clean buffer is", async () => {
  const { access, writes, external, disk } = engine({ "a.py": "a\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", null);
  assert.equal(await session.save(), true);
  view.type("b\n");
  view.dispatch({ changes: { from: 2, to: 4, insert: "" } });
  assert.equal(await session.save(), true);
  assert.equal(writes.length, 0, "nothing to save");

  // Edits undone after someone else changed the file: overwriting must still write the buffer.
  view.type("b\n");
  session.diskChanged("a.py", external("a.py", "theirs\n"));
  view.dispatch({ changes: { from: 2, to: 4, insert: "" } });
  assert.equal(session.getSnapshot().docs[0]?.dirty, false);
  assert.equal(await session.overwrite(), true);
  assert.equal(writes.length, 1);
  assert.equal(disk.get("a.py")?.content, "a\n");
});

test("a save asked for during a save waits for it, then saves only what it did not", async () => {
  const { access, writes, disk } = engine({ "a.py": "a\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", null);
  view.type("b\n");
  const results = [session.save(), session.save()];
  view.type("c\n");
  results.push(session.saveAll(), session.save());
  assert.deepEqual(await Promise.all(results), [true, true, true, true]);
  assert.deepEqual(writes.map((write) => write.expected_revision), ["r1", "r2"]);
  assert.equal(disk.get("a.py")?.content, "a\nb\nc\n");
  assert.deepEqual(session.getSnapshot().docs[0], { path: "a.py", dirty: false, saving: false, issue: null });
});

test("saving every file brings up the first one that could not be saved", async () => {
  const { access, external, disk } = engine({ "a.py": "a\n", "b.py": "b\n", "c.py": "c\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", null);
  view.type("syntax error\n");
  await session.open("b.py", null);
  view.type("mine\n");
  await session.open("c.py", null);
  view.type("fine\n");
  session.diskChanged("b.py", external("b.py", "theirs\n"));

  assert.equal(await session.saveAll(), false);
  const { active, docs } = session.getSnapshot();
  assert.equal(active, "a.py");
  assert.deepEqual(docs.map((doc) => doc.issue?.kind ?? null), ["invalid", "changed", null]);
  assert.equal(disk.get("c.py")?.content, "c\nfine\n");
  assert.equal(view.state.sliceDoc(), "a\nsyntax error\n", "the view shows the file brought up");
});

test("verify catches changes whose events were missed", async () => {
  const { access, external } = engine({ "a.py": "a\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  await session.open("a.py", null);
  external("a.py", "b\n");
  await session.verify();
  await settle();
  assert.equal(view.state.sliceDoc(), "b\n");
});

test("opening a missing file reports it without opening anything", async () => {
  const { access } = engine({});
  const session = new EditorSession(access, () => []);
  await session.open("gone.py", 3);
  assert.deepEqual(session.getSnapshot(), {
    active: null,
    docs: [],
    opening: null,
    openError: { path: "gone.py", message: "not_found happened" },
  });
});

test("the minimal change keeps the common start and end", () => {
  assert.deepEqual(minimalChange(Text.of(["abc", "def"]), Text.of(["abc", "dXf"])), {
    from: 5,
    to: 6,
    insert: Text.of(["X"]),
  });
  assert.equal(minimalChange(Text.of(["same"]), Text.of(["same"])), null);
});

test("closing a file brings up its neighbour, and reopening it reads the disk again", async () => {
  const { access, external } = engine({ "a.py": "a\n", "b.py": "b\n", "c.py": "c\n" });
  const session = new EditorSession(access, () => []);
  const view = port(session);
  session.attach(view);
  for (const path of ["a.py", "b.py", "c.py"]) await session.open(path, null);
  session.switchTo("b.py");
  view.type("unsaved\n");

  assert.equal(session.close("b.py"), true);
  assert.equal(session.getSnapshot().active, "c.py");
  assert.equal(view.state.sliceDoc(), "c\n");
  session.close("c.py");
  assert.deepEqual(session.getSnapshot().docs.map((doc) => doc.path), ["a.py"]);
  assert.equal(session.getSnapshot().active, "a.py");

  external("b.py", "theirs\n");
  await session.open("b.py", null);
  assert.equal(view.state.sliceDoc(), "theirs\n", "the dropped edits are gone");
  session.close("a.py");
  session.close("b.py");
  assert.deepEqual(session.getSnapshot(), { active: null, docs: [], opening: null, openError: null });
});
