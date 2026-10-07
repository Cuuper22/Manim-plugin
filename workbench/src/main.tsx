import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { WorkbenchStore } from "./store/store.ts";
import { StoreContext } from "./store/useWorkbench.ts";
import "./styles.css";

const store = new WorkbenchStore();
void store.connect();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <StoreContext value={store}>
      <App />
    </StoreContext>
  </StrictMode>,
);
