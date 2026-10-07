import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { applyTheme, storedTheme } from "./hooks/useTheme.ts";
import { WorkbenchStore } from "./store/store.ts";
import { StoreContext } from "./store/useWorkbench.ts";
import "./tokens.css";
import "./styles.css";

// Before the first paint, so a chosen theme never flashes the other one.
applyTheme(storedTheme());

const store = new WorkbenchStore();
void store.connect();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <StoreContext value={store}>
      <App />
    </StoreContext>
  </StrictMode>,
);
