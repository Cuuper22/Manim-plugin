import { useEffect } from "react";
import type { Toast } from "../store/reducer.ts";
import { useStore, useWorkbench } from "../store/useWorkbench.ts";
import { Icon } from "./Icon.tsx";

const TOAST_MS = 8000;

export function Toasts() {
  const toasts = useWorkbench((state) => state.toasts);
  return (
    <div className="toasts">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  );
}

function ToastItem({ toast }: { toast: Toast }) {
  const store = useStore();
  useEffect(() => {
    const timer = setTimeout(() => store.dismissToast(toast.id), TOAST_MS);
    return () => clearTimeout(timer);
  }, [store, toast.id]);
  return (
    <div className="card toast" data-tone={toast.tone === "danger" ? "danger" : undefined} role="alert">
      <p>{toast.message}</p>
      <button type="button" className="quiet small icon" aria-label="Dismiss" onClick={() => store.dismissToast(toast.id)}>
        <Icon name="close" />
      </button>
    </div>
  );
}
