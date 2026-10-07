import { createContext, useContext, useSyncExternalStore } from "react";
import type { WorkbenchState } from "./reducer.ts";
import type { WorkbenchStore } from "./store.ts";

export const StoreContext = createContext<WorkbenchStore | null>(null);

export function useStore(): WorkbenchStore {
  const store = useContext(StoreContext);
  if (!store) throw new Error("useStore needs a StoreContext provider.");
  return store;
}

/**
 * Re-renders when the selected value changes. Select stored values as they
 * are: a selector that builds a new object or array on each call makes React
 * loop; derive those with `useMemo` instead.
 */
export function useWorkbench<T>(select: (state: WorkbenchState) => T): T {
  const store = useStore();
  return useSyncExternalStore(store.subscribe, () => select(store.getState()));
}
