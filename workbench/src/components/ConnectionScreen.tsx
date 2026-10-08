import { useEffect, useState, type ReactNode } from "react";
import { engineCommand } from "../engineCommand.ts";
import type { Connection } from "../store/reducer.ts";
import { Icon } from "./Icon.tsx";

const BEHIND_DEV_SERVER = import.meta.env.DEV;
const COPIED_MS = 2000;
const CONNECTING_GRACE_MS = 400;

interface ConnectionScreenProps {
  connection: Exclude<Connection, { status: "online" } | { status: "reconnecting" }>;
  root: string | null;
}

/** What to do while the engine cannot be used; never shows stale data as live. */
export function ConnectionScreen({ connection, root }: ConnectionScreenProps) {
  if (connection.status === "connecting") return <Connecting />;

  const command = <Command text={engineCommand(root, BEHIND_DEV_SERVER)} />;
  // Behind the dev server the engine's link must be opened on this server's address instead.
  const address = BEHIND_DEV_SERVER ? ` with this page's address (${window.location.origin}/?token=…)` : "";
  const openLink = `Then open the Workbench link it prints${address}.`;

  if (connection.status === "unauthorized") {
    // Usually the engine restarted and still holds the port, so starting another one would fail.
    return (
      <Screen title="This page is not signed in">
        <p>
          Pages sign in through the Workbench link the engine prints when it starts, and a restarted engine prints a
          new one. Open the newest link from the engine's terminal in this browser{address}.
        </p>
        <p>If no engine is running, start it:</p>
        {command}
        <p>{openLink}</p>
        <Retrying>Opening the link in another tab signs this page in again, unsaved edits included.</Retrying>
      </Screen>
    );
  }

  return (
    <Screen title="Not connected to the engine">
      <p className="card" data-tone="danger">
        {connection.reason}
      </p>
      <p>Start it from a terminal:</p>
      {command}
      <p>{openLink}</p>
      <Retrying>This page keeps trying to reconnect; unsaved edits stay here meanwhile.</Retrying>
    </Screen>
  );
}

function Retrying({ children }: { children: string }) {
  return (
    <p className="row meta retrying">
      <span className="dot" data-state="running" aria-hidden="true" />
      {children}
    </p>
  );
}

/** Shown only once loading takes noticeably long, so a quick load does not flash a card. */
function Connecting() {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShown(true), CONNECTING_GRACE_MS);
    return () => clearTimeout(timer);
  }, []);
  if (!shown) return null;
  return (
    <Screen title="Connecting to the engine…" busy>
      <p>Loading the project.</p>
    </Screen>
  );
}

function Screen({ title, busy, children }: { title: string; busy?: boolean; children: ReactNode }) {
  return (
    <main className="screen" aria-busy={busy || undefined}>
      <p className="eyebrow">Manim Director</p>
      <h1>{title}</h1>
      {children}
    </main>
  );
}

function Command({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), COPIED_MS);
    return () => clearTimeout(timer);
  }, [copied]);
  const copy = () => {
    navigator.clipboard.writeText(text).then(() => setCopied(true), () => setCopied(false));
  };
  return (
    <div className="command">
      <code className="mono">{text}</code>
      <button type="button" className="quiet small" onClick={copy}>
        <Icon name={copied ? "check" : "copy"} />
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}
