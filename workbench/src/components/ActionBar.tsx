import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { ExportFormat } from "../api/types.ts";
import type { StageAction } from "../model/actions.ts";
import { useDismiss } from "../hooks/useShortcuts.ts";
import { useStore } from "../store/useWorkbench.ts";
import { Icon, type IconName } from "./Icon.tsx";

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

interface ActionSpec {
  action: StageAction;
  label: string;
  icon: IconName;
  keys?: string;
}

/** Make the scene, look at frames of it, check it; Export joins the last group. */
const GROUPS: readonly (readonly ActionSpec[])[] = [
  [
    { action: "preview", label: "Preview", icon: "play", keys: "Meta+Enter Control+Enter" },
    { action: "render", label: "Render", icon: "render" },
  ],
  [
    { action: "still", label: "Still", icon: "still" },
    { action: "frame", label: "Frame at playhead", icon: "frame" },
    { action: "contact_sheet", label: "Contact sheet", icon: "sheet" },
  ],
  [{ action: "qa", label: "QA", icon: "qa" }],
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
      {GROUPS.map((group, index) => (
        <div key={index} className="row action-group">
          {group.map(({ action, label, icon, keys }) => (
            <ActionButton
              key={action}
              label={label}
              icon={icon}
              primary={action === "preview"}
              keys={keys}
              reason={reason(action)}
              state={state(action)}
              onClick={() => onRun(action)}
            />
          ))}
          {index === GROUPS.length - 1 ? <ExportMenu reason={exportReason} state={exportState} onExport={onExport} /> : null}
        </div>
      ))}
    </div>
  );
}

interface ActionButtonProps {
  label: string;
  icon: IconName;
  primary?: boolean;
  keys?: string;
  reason: string | null;
  state: ActionState;
  onClick: () => void;
}

/** A disabled action stays focusable; pressing it says why it is disabled (its title shows only on hover). */
function ActionButton({ label, icon, primary, keys, reason, state, onClick }: ActionButtonProps) {
  const store = useStore();
  return (
    <button
      type="button"
      className={primary ? "primary" : undefined}
      aria-disabled={reason ? true : undefined}
      aria-busy={state.busy || undefined}
      aria-keyshortcuts={keys}
      title={reason ?? undefined}
      onClick={() => (reason === null ? onClick() : store.hint(`${label}: ${reason}`))}
    >
      <Icon name={icon} />
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
  const store = useStore();
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
        <Icon name="export" />
        Export
        <Icon name="chevron" />
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
                    if (why) return store.hint(`${label}: ${why}`);
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
