import { useEffect } from "react";
import type { Toast } from "../store/reducer.ts";
import { useStore, useWorkbench } from "../store/useWorkbench.ts";

const TOAST_MS = 8000;

export function Toasts() {
  const toasts = useWorkbench((state) => state.toasts);
  return (
    <ul className="toasts">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </ul>
  );
}

function ToastItem({ toast }: { toast: Toast }) {
  const store = useStore();
  useEffect(() => {
    const timer = setTimeout(() => store.dismissToast(toast.id), TOAST_MS);
    return () => clearTimeout(timer);
  }, [store, toast.id]);
  return (
    <li className="card toast" data-tone="danger" role="alert">
      <p>{toast.message}</p>
      <button type="button" aria-label="Dismiss" onClick={() => store.dismissToast(toast.id)}>
        ×
      </button>
    </li>
  );
}
