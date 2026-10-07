import { lazy, Suspense, type ComponentProps, type ReactNode } from "react";
import { TabList, panelId, tabId } from "./TabList.tsx";

// CodeMirror is most of the bundle; it loads after the first paint.
const CodeEditor = lazy(() => import("../editor/CodeEditor.tsx"));

export type InspectorTab = "code" | "findings";

interface InspectorProps {
  tab: InspectorTab;
  onTab: (tab: InspectorTab) => void;
  active: boolean;
  editor: ComponentProps<typeof CodeEditor>;
  findingCount: number;
  /** The findings pane. */
  children: ReactNode;
}

const panel = (id: InspectorTab) => ({
  id: panelId("inspector", id),
  role: "tabpanel",
  "aria-labelledby": tabId("inspector", id),
});

export function Inspector({ tab, onTab, active, editor, findingCount, children }: InspectorProps) {
  const tabs = [
    { id: "code" as const, label: "Code" },
    {
      id: "findings" as const,
      label: (
        <>
          Findings <span className="meta">{findingCount}</span>
        </>
      ),
    },
  ];
  return (
    <aside
      className="region region-inspector"
      id="region-inspector"
      aria-label="Code and findings"
      data-active={active || undefined}
    >
      <header className="bar">
        <TabList label="Inspector" idPrefix="inspector" tabs={tabs} selected={tab} onSelect={onTab} />
      </header>
      <div className="inspector-panel" {...panel("code")} hidden={tab !== "code"}>
        <Suspense fallback={<p className="pane-note">Loading the editor…</p>}>
          <CodeEditor {...editor} />
        </Suspense>
      </div>
      <div className="inspector-panel inspector-findings" {...panel("findings")} hidden={tab !== "findings"}>
        {children}
      </div>
    </aside>
  );
}
