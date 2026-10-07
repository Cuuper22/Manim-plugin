import { ConnectionScreen } from "./components/ConnectionScreen.tsx";
import { Toasts } from "./components/Toasts.tsx";
import { Workbench } from "./components/Workbench.tsx";
import { useWorkbench } from "./store/useWorkbench.ts";

export function App() {
  const connection = useWorkbench((state) => state.connection);
  const root = useWorkbench((state) => state.root);
  const workspace = useWorkbench((state) => state.workspace);
  const usable = connection.status === "online" || connection.status === "reconnecting";

  return (
    <>
      {usable ? null : <ConnectionScreen connection={connection} root={root} />}
      {/* Kept, hidden, while the engine cannot be used: unsaved edits and the view are still there when it
          is back. Another project served at the same address starts over. */}
      {workspace ? (
        <Workbench
          key={workspace.project.root}
          workspace={workspace}
          reconnecting={connection.status === "reconnecting"}
          suspended={!usable}
        />
      ) : null}
      <Toasts />
    </>
  );
}
