import { engineCommand } from "../engineCommand.ts";
import type { Connection } from "../store/reducer.ts";

const BEHIND_DEV_SERVER = import.meta.env.DEV;

interface ConnectionScreenProps {
  connection: Exclude<Connection, { status: "online" } | { status: "reconnecting" }>;
  root: string | null;
}

/** What to do while the engine cannot be used; never shows stale data as live. */
export function ConnectionScreen({ connection, root }: ConnectionScreenProps) {
  if (connection.status === "connecting") {
    return (
      <main className="screen" aria-busy="true">
        <h1>Connecting to the engine…</h1>
      </main>
    );
  }

  const command = <code className="command mono">{engineCommand(root, BEHIND_DEV_SERVER)}</code>;
  // Behind the dev server the engine's link must be opened on this server's address instead.
  const openLink = BEHIND_DEV_SERVER
    ? `Open its Workbench link with this page's address (${window.location.origin}/?token=…).`
    : "Open the Workbench link it prints.";

  if (connection.status === "unauthorized") {
    return (
      <main className="screen">
        <h1>This page is not signed in</h1>
        <p>
          It signs in through the Workbench link the engine prints when it starts, and a restarted engine prints a new
          one. Use the latest link, or start the engine with
        </p>
        {command}
        <p>{openLink}</p>
      </main>
    );
  }

  return (
    <main className="screen">
      <h1>Not connected to the engine</h1>
      <p>{connection.reason}</p>
      <p>Start it with</p>
      {command}
      <p>{openLink} Until then this page keeps trying to reconnect.</p>
    </main>
  );
}
