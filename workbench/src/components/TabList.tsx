import { useLayoutEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import { Icon } from "./Icon.tsx";

export interface TabSpec<Id extends string> {
  id: Id;
  label: ReactNode;
}

interface TabListProps<Id extends string> {
  label: string;
  /** Tab `x` is `<idPrefix>-tab-x` and controls `<idPrefix>-panel-x`. */
  idPrefix: string;
  tabs: readonly TabSpec<Id>[];
  selected: Id;
  onSelect: (id: Id) => void;
  /** Tabs that can close show a × and close on Delete or a middle click; `true` when it closed. */
  onClose?: (id: Id) => boolean;
  className?: string;
}

export const tabId = (prefix: string, id: string) => `${prefix}-tab-${id}`;
export const panelId = (prefix: string, id: string) => `${prefix}-panel-${id}`;

/** WAI-ARIA tabs with automatic activation: arrows, Home and End move between tabs. */
export function TabList<Id extends string>({ label, idPrefix, tabs, selected, onSelect, onClose, className }: TabListProps<Id>) {
  const list = useRef<HTMLDivElement>(null);
  /** A tab closed from the keyboard hands focus to the one selected next. */
  const refocus = useRef(false);
  useLayoutEffect(() => {
    if (!refocus.current) return;
    refocus.current = false;
    list.current?.querySelector<HTMLElement>(`#${CSS.escape(tabId(idPrefix, selected))}`)?.focus();
  }, [idPrefix, selected]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Delete" && onClose) {
      event.preventDefault();
      refocus.current = onClose(selected);
      return;
    }
    const index = tabs.findIndex((tab) => tab.id === selected);
    const moves: Record<string, number> = { ArrowRight: index + 1, ArrowLeft: index - 1, Home: 0, End: tabs.length - 1 };
    const target = moves[event.key];
    if (target === undefined) return;
    event.preventDefault();
    const next = tabs[(target + tabs.length) % tabs.length]!;
    list.current?.querySelector<HTMLElement>(`#${CSS.escape(tabId(idPrefix, next.id))}`)?.focus();
    onSelect(next.id);
  };

  return (
    <div ref={list} role="tablist" aria-label={label} className={className ?? "tabs"} onKeyDown={onKeyDown}>
      {tabs.map((tab) => {
        const isSelected = tab.id === selected;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            id={tabId(idPrefix, tab.id)}
            aria-controls={panelId(idPrefix, tab.id)}
            aria-selected={isSelected}
            tabIndex={isSelected ? 0 : -1}
            aria-keyshortcuts={onClose ? "Delete" : undefined}
            onClick={() => onSelect(tab.id)}
            onAuxClick={(event) => event.button === 1 && onClose?.(tab.id)}
          >
            {tab.label}
            {onClose ? (
              // Not a button of its own (no controls inside a tab): Delete closes the focused tab from the keyboard.
              <span
                className="tab-close"
                aria-hidden="true"
                title="Close"
                onClick={(event) => {
                  event.stopPropagation();
                  onClose(tab.id);
                }}
              >
                <Icon name="close" />
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
