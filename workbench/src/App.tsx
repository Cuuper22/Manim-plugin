import { ConnectionScreen } from "./components/ConnectionScreen.tsx";
import { Overview } from "./components/Overview.tsx";
import { Toasts } from "./components/Toasts.tsx";
import { useWorkbench } from "./store/useWorkbench.ts";

export function App() {
  const connection = useWorkbench((state) => state.connection);
  const root = useWorkbench((state) => state.root);
  const workspace = useWorkbench((state) => state.workspace);
  const jobs = useWorkbench((state) => state.jobs);

  return (
    <>
      {connection.status === "online" || connection.status === "reconnecting" ? (
        workspace ? <Overview workspace={workspace} jobs={jobs} reconnecting={connection.status === "reconnecting"} /> : null
      ) : (
        <ConnectionScreen connection={connection} root={root} />
      )}
      <Toasts />
    </>
  );
}
