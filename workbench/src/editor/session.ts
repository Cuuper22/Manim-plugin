import { EditorState, type Extension, type Text, type TransactionSpec } from "@codemirror/state";
import type { SourceDocument } from "../api/sourcePaging.ts";
import type { SourceWrite, SourceWriteResult } from "../api/types.ts";
import type { Outcome } from "../store/store.ts";

/** Why the open document cannot simply be saved over its file. */
export type DocIssue =
  /** The file changed on disk while the buffer has edits. */
  | { kind: "changed" }
  | { kind: "deleted" }
  /** The engine refused the last save, e.g. a Python syntax error. */
  | { kind: "invalid"; message: string; line: number | null };

export interface DocStatus {
  path: string;
  dirty: boolean;
  saving: boolean;
  issue: DocIssue | null;
}

export interface SessionSnapshot {
  active: string | null;
  /** In opening order. */
  docs: readonly DocStatus[];
  /** The path being loaded. */
  opening: string | null;
  openError: { path: string; message: string } | null;
}

/** The engine's source API as the session uses it; failures are already reported. */
export interface SourceAccess {
  /** `quiet`: the caller shows the failure itself. */
  load(path: string, quiet: boolean): Promise<Outcome<SourceDocument>>;
  write(write: SourceWrite): Promise<Outcome<SourceWriteResult>>;
  /** The file's current revision; `null` when it does not exist. */
  revision(path: string): Promise<Outcome<string | null>>;
}

/** The one editor view; the session swaps documents in and out of it. */
export interface EditorPort {
  readonly state: EditorState;
  setState(state: EditorState): void;
  dispatch(spec: TransactionSpec): void;
  reveal(line: number, focus: boolean): void;
}

export type ExtensionsFor = (source: SourceDocument) => Extension;

interface OpenDoc extends Omit<DocStatus, "saving"> {
  /** The save in flight; `saving` while set. */
  write: Promise<unknown> | null;
  /** The disk revision the buffer is based on; `null` when the file is gone. */
  revision: string | null;
  /** The buffer's text at `revision`. */
  saved: Text;
  state: EditorState;
}

/**
 * Open source documents and their revision-safe saves (CONTRACT-http §6.3,
 * §13.2): loads are byte-exact, saves send the revision they started from,
 * and a file that moved on is reloaded only while the buffer is clean.
 */
export class EditorSession {
  readonly #access: SourceAccess;
  readonly #extensions: ExtensionsFor;
  readonly #docs = new Map<string, OpenDoc>();
  /** The newest revision announced per path; `null` = deleted. */
  readonly #disk = new Map<string, string | null>();
  readonly #listeners = new Set<() => void>();
  #port: EditorPort | null = null;
  #active: OpenDoc | null = null;
  #pendingReveal: { line: number; focus: boolean } | null = null;
  #opening: string | null = null;
  #openError: SessionSnapshot["openError"] = null;
  /** Bumped per `open`; a slower earlier load is dropped. */
  #ticket = 0;
  #snapshot: SessionSnapshot = { active: null, docs: [], opening: null, openError: null };

  constructor(access: SourceAccess, extensions: ExtensionsFor) {
    this.#access = access;
    this.#extensions = extensions;
  }

  readonly getSnapshot = (): SessionSnapshot => this.#snapshot;

  readonly subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  attach(port: EditorPort | null): void {
    this.#port = port;
    if (port && this.#active) port.setState(this.#active.state);
    this.#flushReveal();
  }

  /** Keeps the active document in step with the view; call from the view's update listener. */
  viewChanged(state: EditorState): void {
    const doc = this.#active;
    if (!doc || doc.state === state) return;
    doc.state = state;
    const dirty = !state.doc.eq(doc.saved);
    if (dirty === doc.dirty) return;
    doc.dirty = dirty;
    this.#emit();
  }

  async open(path: string, line: number | null, focus = false): Promise<void> {
    const ticket = ++this.#ticket;
    this.#opening = null;
    this.#openError = null;
    let doc = this.#docs.get(path);
    if (!doc) {
      this.#opening = path;
      this.#emit();
      const loaded = await this.#access.load(path, true);
      if (ticket !== this.#ticket) return;
      this.#opening = null;
      if (!loaded.ok) {
        this.#openError = { path, message: loaded.error.message };
        this.#emit();
        return;
      }
      doc = this.#create(loaded.value);
      this.#docs.set(path, doc);
    }
    this.#activate(doc);
    this.#emit();
    if (line !== null) {
      this.#pendingReveal = { line, focus };
      this.#flushReveal();
    }
  }

  switchTo(path: string): void {
    const doc = this.#docs.get(path);
    if (!doc) return;
    this.#activate(doc);
    this.#emit();
  }

  /**
   * Closes a document, dropping any unsaved edits; the next one, else the previous, becomes active.
   * `false` while it is being saved.
   */
  close(path: string): boolean {
    const doc = this.#docs.get(path);
    if (!doc) return true;
    if (doc.write) return false;
    const paths = [...this.#docs.keys()];
    const index = paths.indexOf(path);
    const neighbour = paths[index + 1] ?? paths[index - 1];
    this.#docs.delete(path);
    this.#disk.delete(path);
    if (this.#active === doc) {
      this.#active = null;
      if (neighbour !== undefined) this.#activate(this.#docs.get(neighbour)!);
    }
    this.#emit();
    return true;
  }

  /**
   * Saves the active document if it has anything to save.
   * `false` when it was not saved (the reason is on the document or a toast).
   */
  save(): Promise<boolean> {
    const doc = this.#active;
    return doc && (doc.dirty || doc.issue) ? this.#save(doc) : Promise.resolve(true);
  }

  /**
   * Saves every document with unsaved edits, e.g. before a render reads the files.
   * The first one that could not be saved becomes the active one, so its banner says why.
   */
  async saveAll(): Promise<boolean> {
    let failed: OpenDoc | null = null;
    for (const doc of this.#docs.values()) {
      if (doc.dirty && !(await this.#save(doc))) failed ??= doc;
    }
    if (failed) {
      this.#activate(failed);
      this.#emit();
    }
    return failed === null;
  }

  /** Replaces the buffer with the file on disk; undo brings the edits back. */
  async reload(): Promise<void> {
    if (this.#active) await this.#reload(this.#active, true);
  }

  /** Saves the buffer over whatever is on disk now, checked against its current revision. */
  async overwrite(): Promise<boolean> {
    const doc = this.#active;
    if (!doc || doc.write) return false;
    const current = await this.#access.revision(doc.path);
    if (!current.ok) return false;
    this.#disk.set(doc.path, current.value);
    doc.revision = current.value;
    doc.issue = null;
    return this.#save(doc);
  }

  /** A `file` event or a re-check reported the file at `revision`. */
  diskChanged(path: string, revision: string | null): void {
    this.#disk.set(path, revision);
    const doc = this.#docs.get(path);
    if (doc) this.#reconcile(doc);
  }

  /** Re-reads every open file's revision, e.g. after a snapshot that may hide missed events. */
  async verify(): Promise<void> {
    await Promise.all(
      [...this.#docs.keys()].map(async (path) => {
        const current = await this.#access.revision(path);
        if (current.ok) this.diskChanged(path, current.value);
      }),
    );
  }

  #create(source: SourceDocument): OpenDoc {
    const state = this.#newState(source);
    this.#disk.set(source.path, source.revision);
    return {
      path: source.path,
      dirty: false,
      write: null,
      issue: null,
      revision: source.revision,
      saved: state.doc,
      state,
    };
  }

  #newState(source: SourceDocument): EditorState {
    return EditorState.create({
      doc: source.content,
      // Splitting on exactly the file's line ending keeps any other "\r" in its line, so saves are byte-exact.
      extensions: [EditorState.lineSeparator.of(source.eol === "crlf" ? "\r\n" : "\n"), this.#extensions(source)],
    });
  }

  #activate(doc: OpenDoc): void {
    if (this.#active === doc) return;
    this.#active = doc;
    this.#port?.setState(doc.state);
  }

  #flushReveal(): void {
    if (!this.#port || !this.#active || !this.#pendingReveal) return;
    const { line, focus } = this.#pendingReveal;
    this.#pendingReveal = null;
    this.#port.reveal(line, focus);
  }

  async #save(doc: OpenDoc): Promise<boolean> {
    if (doc.write) {
      // One write per file at a time: wait for the one in flight, which may have written this very text.
      while (doc.write) await doc.write;
      if (!doc.dirty && doc.issue === null) return true;
    }
    if (doc.issue?.kind === "changed") return false;
    const text = doc.state.doc;
    // Recreating a deleted file must not find one there.
    const expected = doc.issue?.kind === "deleted" ? null : doc.revision;
    const write = this.#access.write({
      path: doc.path,
      expected_revision: expected,
      edit: { kind: "replace_all", content: doc.state.sliceDoc() },
    });
    doc.write = write;
    this.#emit();
    let outcome: Outcome<SourceWriteResult>;
    try {
      outcome = await write;
    } finally {
      doc.write = null;
    }
    if (outcome.ok) {
      // The disk now holds this save, unless an event already reported it or a newer write. Only indexed files
      // get `file` events, and they may come after the response.
      if (this.#disk.get(doc.path) === expected) this.#disk.set(doc.path, outcome.value.revision);
      doc.revision = outcome.value.revision;
      doc.saved = text;
      doc.dirty = !doc.state.doc.eq(text);
      doc.issue = null;
    } else if (outcome.error.code === "revision_conflict") {
      const current = outcome.error.data?.current_revision;
      this.#disk.set(doc.path, typeof current === "string" ? current : null);
      doc.issue = current ? { kind: "changed" } : { kind: "deleted" };
    } else if (outcome.error.code === "source_invalid") {
      const line = outcome.error.data?.line;
      doc.issue = { kind: "invalid", message: outcome.error.message, line: typeof line === "number" ? line : null };
    }
    this.#emit();
    if (outcome.ok) this.#reconcile(doc);
    return outcome.ok;
  }

  #reconcile(doc: OpenDoc): void {
    const disk = this.#disk.get(doc.path);
    if (doc.write || disk === undefined) return;
    if (disk === doc.revision) {
      if (doc.issue?.kind === "changed" || doc.issue?.kind === "deleted") this.#setIssue(doc, null);
    } else if (disk === null) {
      this.#setIssue(doc, { kind: "deleted" });
    } else if (doc.dirty) {
      this.#setIssue(doc, { kind: "changed" });
    } else {
      void this.#reload(doc, false);
    }
  }

  /** `discard`: the user chose to drop their edits; otherwise edits made while loading win. */
  async #reload(doc: OpenDoc, discard: boolean): Promise<void> {
    const loaded = await this.#access.load(doc.path, false);
    if (!loaded.ok) return;
    if (!discard && doc.dirty) {
      this.#setIssue(doc, { kind: "changed" });
      return;
    }
    const source = loaded.value;
    if (doc.state.lineBreak !== (source.eol === "crlf" ? "\r\n" : "\n")) {
      doc.state = this.#newState(source);
      if (this.#active === doc) this.#port?.setState(doc.state);
    } else {
      const next = doc.state.toText(source.content);
      const change = minimalChange(doc.state.doc, next);
      if (change) this.#apply(doc, { changes: change });
    }
    this.#disk.set(doc.path, source.revision);
    doc.revision = source.revision;
    doc.saved = doc.state.doc;
    doc.dirty = false;
    doc.issue = null;
    this.#emit();
  }

  #apply(doc: OpenDoc, spec: TransactionSpec): void {
    if (this.#active === doc && this.#port) {
      this.#port.dispatch(spec);
      doc.state = this.#port.state;
    } else {
      doc.state = doc.state.update(spec).state;
    }
  }

  #setIssue(doc: OpenDoc, issue: DocIssue | null): void {
    doc.issue = issue;
    this.#emit();
  }

  #emit(): void {
    this.#snapshot = {
      active: this.#active?.path ?? null,
      docs: [...this.#docs.values()].map(({ path, dirty, write, issue }) => ({ path, dirty, saving: write !== null, issue })),
      opening: this.#opening,
      openError: this.#openError,
    };
    for (const listener of this.#listeners) listener();
  }
}

/** The one replacement that turns `from` into `to`, keeping the common start and end (and so the cursor). */
export function minimalChange(from: Text, to: Text): { from: number; to: number; insert: Text } | null {
  const a = from.toString();
  const b = to.toString();
  if (a === b) return null;
  const shorter = Math.min(a.length, b.length);
  let start = 0;
  while (start < shorter && a.charCodeAt(start) === b.charCodeAt(start)) start += 1;
  let end = 0;
  while (end < shorter - start && a.charCodeAt(a.length - 1 - end) === b.charCodeAt(b.length - 1 - end)) end += 1;
  return { from: start, to: a.length - end, insert: to.slice(start, b.length - end) };
}
