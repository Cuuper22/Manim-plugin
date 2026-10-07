import { ConnectionScreen } from "./components/ConnectionScreen.tsx";
import { Toasts } from "./components/Toasts.tsx";
import { Workbench } from "./components/Workbench.tsx";
import { useWorkbench } from "./store/useWorkbench.ts";

export function App() {
  const connection = useWorkbench((state) => state.connection);
  const root = useWorkbench((state) => state.root);
  const workspace = useWorkbench((state) => state.workspace);

  let content = null;
  if (connection.status !== "online" && connection.status !== "reconnecting") {
    content = <ConnectionScreen connection={connection} root={root} />;
  } else if (workspace) {
    content = <Workbench workspace={workspace} reconnecting={connection.status === "reconnecting"} />;
  }
  return (
    <>
      {content}
      <Toasts />
    </>
  );
}
