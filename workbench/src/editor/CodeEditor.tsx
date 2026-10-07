import { EditorState, Prec } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { useEffect, useImperativeHandle, useRef, useState, useSyncExternalStore, type Ref, type RefObject } from "react";
import { CodeSkeleton } from "../components/CodeSkeleton.tsx";
import { TabList, panelId, tabId } from "../components/TabList.tsx";
import { baseName } from "../model/format.ts";
import type { WorkbenchStore } from "../store/store.ts";
import { useStore, useWorkbench } from "../store/useWorkbench.ts";
import { baseExtensions, documentExtensions } from "./codemirror.ts";
import { EditorSession, type DocStatus, type EditorPort, type SourceAccess } from "./session.ts";

export interface EditorApi {
  save(): Promise<boolean>;
  /** Saves every edited file; `false` if any could not be saved. */
  saveAll(): Promise<boolean>;
}

/** Open `path` (scrolled to `line`); a new `seq` repeats the same request. */
export interface OpenRequest {
  path: string;
  line: number | null;
  focus: boolean;
  seq: number;
}

interface CodeEditorProps {
  request: OpenRequest | null;
  api: Ref<EditorApi>;
  onPreview: () => void;
}

/** Load failures shown inline instead of as a toast. */
const OPEN_ERRORS = ["not_found", "invalid_path", "not_utf8", "file_too_large", "unsupported_file_type", "source_incomplete"];

function sourceAccess(store: WorkbenchStore): SourceAccess {
  return {
    load: (path, quiet) => store.perform(`source:${path}`, (client) => client.loadSource(path), quiet ? OPEN_ERRORS : []),
    write: (write) =>
      store.perform(`save:${write.path}`, (client) => client.writeSource(write), ["revision_conflict", "source_invalid"]),
    revision: async (path) => {
      const outcome = await store.perform(`revision:${path}`, (client) => client.sourcePage(path, 1, 1), ["not_found"]);
      if (outcome.ok) return { ok: true, value: outcome.value.revision };
      return outcome.error.code === "not_found" ? { ok: true, value: null } : outcome;
    },
  };
}

function createSession(store: WorkbenchStore, preview: RefObject<() => void>): EditorSession {
  const session: EditorSession = new EditorSession(sourceAccess(store), (source) => [
    documentExtensions(source),
    Prec.highest(
      keymap.of([
        { key: "Mod-s", preventDefault: true, run: () => (void session.save(), true) },
        { key: "Mod-Enter", preventDefault: true, run: () => (preview.current(), true) },
      ]),
    ),
    EditorView.updateListener.of((update) => session.viewChanged(update.state)),
  ]);
  return session;
}

function portFor(view: EditorView): EditorPort {
  return {
    get state() {
      return view.state;
    },
    setState: (state) => view.setState(state),
    dispatch: (spec) => view.dispatch(spec),
    reveal: (line, focus) => {
      const { doc } = view.state;
      const target = doc.line(Math.min(Math.max(line, 1), doc.lines));
      view.dispatch({
        selection: { anchor: target.from },
        effects: EditorView.scrollIntoView(target.from, { y: "start", yMargin: 48 }),
      });
      if (focus) view.focus();
    },
  };
}

/** CodeMirror over the engine's source API: byte-exact loads, revision-checked saves, conflicts surfaced. */
export default function CodeEditor({ request, api, onPreview }: CodeEditorProps) {
  const store = useStore();
  const preview = useRef(onPreview);
  preview.current = onPreview;
  const [session] = useState(() => createSession(store, preview));
  const snapshot = useSyncExternalStore(session.subscribe, session.getSnapshot);
  const host = useRef<HTMLDivElement>(null);

  useImperativeHandle(api, () => ({ save: () => session.save(), saveAll: () => session.saveAll() }), [session]);

  useEffect(() => {
    const view = new EditorView({ parent: host.current!, state: EditorState.create({ extensions: baseExtensions }) });
    session.attach(portFor(view));
    // A banner above the editor shrinks it; keep the line being typed on in view.
    const resized = new ResizeObserver(() => {
      if (view.hasFocus) view.dispatch({ effects: EditorView.scrollIntoView(view.state.selection.main.head) });
    });
    resized.observe(host.current!);
    return () => {
      resized.disconnect();
      session.attach(null);
      view.destroy();
    };
  }, [session]);

  useEffect(() => {
    if (request) void session.open(request.path, request.line, request.focus);
  }, [session, request]);

  useDiskSync(session);
  useLeaveGuard(snapshot.docs.some((doc) => doc.dirty));

  const active = snapshot.docs.find((doc) => doc.path === snapshot.active) ?? null;
  return (
    <div className="code">
      <div className="bar">
        {snapshot.docs.length > 0 && active ? (
          <TabList
            label="Open files"
            idPrefix="file"
            className="tabs file-tabs"
            tabs={snapshot.docs.map((doc) => ({
              id: doc.path,
              label: (
                <span title={doc.path}>
                  {baseName(doc.path)}
                  {doc.dirty ? (
                    <>
                      <span aria-hidden="true"> ●</span>
                      <span className="visually-hidden"> (unsaved)</span>
                    </>
                  ) : null}
                </span>
              ),
            }))}
            selected={active.path}
            onSelect={(path) => session.switchTo(path)}
          />
        ) : (
          <span className="muted">{snapshot.opening ? `Opening ${snapshot.opening}…` : "No file open"}</span>
        )}
        {active ? <span className="meta">{active.saving ? "Saving…" : active.dirty ? "Unsaved" : "Saved"}</span> : null}
        <button
          type="button"
          disabled={!active || active.saving || (!active.dirty && active.issue === null)}
          aria-keyshortcuts="Meta+S Control+S"
          onClick={() => void session.save()}
        >
          Save
        </button>
      </div>
      {active ? <IssueBanner doc={active} session={session} /> : null}
      {snapshot.openError ? (
        <p className="card banner" data-tone="danger" role="alert">
          Could not open {snapshot.openError.path}: {snapshot.openError.message}
        </p>
      ) : null}
      {!active && snapshot.opening ? <CodeSkeleton label={`Opening ${snapshot.opening}`} /> : null}
      <div
        ref={host}
        className="code-host"
        role="tabpanel"
        id={active ? panelId("file", active.path) : undefined}
        aria-labelledby={active ? tabId("file", active.path) : undefined}
        hidden={!active}
      />
    </div>
  );
}

function IssueBanner({ doc, session }: { doc: DocStatus; session: EditorSession }) {
  const issue = doc.issue;
  if (!issue) return null;
  if (issue.kind === "changed") {
    return (
      <div className="card banner" data-tone="warning" role="alert">
        <p>{doc.path} changed on disk while you were editing. Your edits are still here.</p>
        <button type="button" onClick={() => void session.reload()}>
          Reload from disk
        </button>
        <button type="button" onClick={() => void session.overwrite()}>
          Overwrite with mine
        </button>
        <p className="muted">After a reload, Undo brings your edits back.</p>
      </div>
    );
  }
  if (issue.kind === "deleted") {
    return (
      <div className="card banner" data-tone="warning" role="alert">
        <p>{doc.path} was deleted on disk.</p>
        <button type="button" onClick={() => void session.save()}>
          Save to recreate it
        </button>
      </div>
    );
  }
  return (
    <div className="card banner" data-tone="danger" role="alert">
      <p>Not saved: {issue.message}</p>
      {issue.line !== null ? (
        <button type="button" onClick={() => void session.open(doc.path, issue.line, true)}>
          Go to line {issue.line}
        </button>
      ) : null}
    </div>
  );
}

/** While edits are unsaved, the browser asks before the tab closes or navigates away (e.g. to a new sign-in link). */
function useLeaveGuard(unsaved: boolean): void {
  useEffect(() => {
    if (!unsaved) return;
    const ask = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", ask);
    return () => window.removeEventListener("beforeunload", ask);
  }, [unsaved]);
}

/** Forwards `file` events, and re-checks open files after every snapshot (events may have been missed). */
function useDiskSync(session: EditorSession): void {
  const fileRevisions = useWorkbench((state) => state.fileRevisions);
  const snapshots = useWorkbench((state) => state.snapshots);
  const forwarded = useRef(fileRevisions);
  const verified = useRef(snapshots);

  useEffect(() => {
    for (const [path, revision] of Object.entries(fileRevisions)) {
      if (forwarded.current[path] !== revision) session.diskChanged(path, revision);
    }
    forwarded.current = fileRevisions;
  }, [session, fileRevisions]);

  useEffect(() => {
    if (verified.current === snapshots) return;
    verified.current = snapshots;
    void session.verify();
  }, [session, snapshots]);
}
