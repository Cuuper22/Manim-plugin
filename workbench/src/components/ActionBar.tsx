import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ExportFormat } from "../api/types.ts";
import type { StageAction } from "../model/actions.ts";
import { useDismiss } from "../hooks/useShortcuts.ts";

export interface ActionState {
  busy: boolean;
  /** Progress of its job in `[0, 1]`, `null` while unknown. */
  fraction: number | null;
}

interface ActionBarProps {
  /** Why an action cannot run now, or `null`. */
  reason: (action: StageAction) => string | null;
  state: (action: StageAction) => ActionState;
  exportReason: (format: ExportFormat) => string | null;
  exportState: ActionState;
  onRun: (action: StageAction) => void;
  onExport: (format: ExportFormat) => void;
}

const ACTIONS: readonly { action: StageAction; label: string; keys?: string }[] = [
  { action: "preview", label: "Preview", keys: "Meta+Enter Control+Enter" },
  { action: "render", label: "Render" },
  { action: "still", label: "Still" },
  { action: "frame", label: "Frame at playhead" },
  { action: "contact_sheet", label: "Contact sheet" },
  { action: "qa", label: "QA" },
];

const EXPORTS: readonly { format: ExportFormat; label: string }[] = [
  { format: "mp4", label: "MP4 video" },
  { format: "webm", label: "WebM video" },
  { format: "gif", label: "GIF loop" },
  { format: "zip", label: "Project bundle (.zip)" },
];

/** The job buttons of the selected scene, each busy while its request or job runs. */
export function ActionBar({ reason, state, exportReason, exportState, onRun, onExport }: ActionBarProps) {
  return (
    <div className="row actions" role="group" aria-label="Scene actions">
      {ACTIONS.map(({ action, label, keys }) => (
        <ActionButton
          key={action}
          label={label}
          primary={action === "preview"}
          keys={keys}
          reason={reason(action)}
          state={state(action)}
          onClick={() => onRun(action)}
        />
      ))}
      <ExportMenu reason={exportReason} state={exportState} onExport={onExport} />
    </div>
  );
}

interface ActionButtonProps {
  label: string;
  primary?: boolean;
  keys?: string;
  reason: string | null;
  state: ActionState;
  onClick: () => void;
}

function ActionButton({ label, primary, keys, reason, state, onClick }: ActionButtonProps) {
  return (
    <button
      type="button"
      className={primary ? "primary" : undefined}
      aria-disabled={reason ? true : undefined}
      aria-busy={state.busy || undefined}
      aria-keyshortcuts={keys}
      title={reason ?? undefined}
      onClick={() => reason === null && onClick()}
    >
      {label}
      {state.busy ? <BusyBar fraction={state.fraction} /> : null}
    </button>
  );
}

/** The busy state is announced by the job tray; this bar is only its picture. */
function BusyBar({ fraction }: { fraction: number | null }) {
  return (
    <span className="busy" aria-hidden="true" data-indeterminate={fraction === null || undefined}>
      <span style={fraction === null ? undefined : { width: `${fraction * 100}%` }} />
    </span>
  );
}

interface ExportMenuProps {
  reason: (format: ExportFormat) => string | null;
  state: ActionState;
  onExport: (format: ExportFormat) => void;
}

function ExportMenu({ reason, state, onExport }: ExportMenuProps) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const toggle = useRef<HTMLButtonElement>(null);
  const close = () => {
    setOpen(false);
    toggle.current?.focus();
  };
  useDismiss(open, close, root);

  useEffect(() => {
    if (open) root.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
  }, [open]);

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const items = [...(root.current?.querySelectorAll<HTMLElement>("[role=menuitem]") ?? [])];
    const index = items.indexOf(document.activeElement as HTMLElement);
    const moves: Record<string, number> = { ArrowDown: index + 1, ArrowUp: index - 1, Home: 0, End: items.length - 1 };
    const target = moves[event.key];
    if (target !== undefined) {
      event.preventDefault();
      items[(target + items.length) % items.length]?.focus();
    } else if (event.key === "Tab") {
      setOpen(false);
    }
  };

  return (
    <div className="menu-anchor" ref={root}>
      <button
        ref={toggle}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-busy={state.busy || undefined}
        onClick={() => setOpen((value) => !value)}
      >
        Export
        <svg className="stroke" viewBox="0 0 16 16" aria-hidden="true">
          <path d="M4 6l4 4 4-4" />
        </svg>
        {state.busy ? <BusyBar fraction={state.fraction} /> : null}
      </button>
      {open ? (
        <ul className="popover" role="menu" aria-label="Export" onKeyDown={onKeyDown}>
          {EXPORTS.map(({ format, label }) => {
            const why = reason(format);
            return (
              <li key={format} role="none">
                <button
                  type="button"
                  role="menuitem"
                  tabIndex={-1}
                  aria-disabled={why ? true : undefined}
                  title={why ?? undefined}
                  onClick={() => {
                    if (why) return;
                    close();
                    onExport(format);
                  }}
                >
                  {label}
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}
