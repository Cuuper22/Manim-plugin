import { useRef, type KeyboardEvent, type ReactNode } from "react";

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
  className?: string;
}

export const tabId = (prefix: string, id: string) => `${prefix}-tab-${id}`;
export const panelId = (prefix: string, id: string) => `${prefix}-panel-${id}`;

/** WAI-ARIA tabs with automatic activation: arrows, Home and End move between tabs. */
export function TabList<Id extends string>({ label, idPrefix, tabs, selected, onSelect, className }: TabListProps<Id>) {
  const list = useRef<HTMLDivElement>(null);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
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
            onClick={() => onSelect(tab.id)}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
