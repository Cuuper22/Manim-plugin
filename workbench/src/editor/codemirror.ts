import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { pythonLanguage } from "@codemirror/lang-python";
import { bracketMatching, HighlightStyle, indentOnInput, indentUnit, syntaxHighlighting } from "@codemirror/language";
import { EditorState, type Extension } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import type { SourceDocument } from "../api/sourcePaging.ts";

// Colors are the workbench's tokens, so the editor follows the light and dark themes.
const highlight = HighlightStyle.define([
  {
    tag: [
      tags.keyword,
      tags.controlKeyword,
      tags.definitionKeyword,
      tags.moduleKeyword,
      tags.operatorKeyword,
      tags.self,
      tags.bool,
      tags.null,
    ],
    color: "var(--accent)",
  },
  { tag: [tags.string, tags.special(tags.string), tags.escape], color: "var(--success)" },
  { tag: tags.number, color: "var(--warning)" },
  { tag: [tags.comment, tags.meta], color: "var(--text-muted)", fontStyle: "italic" },
  { tag: [tags.function(tags.definition(tags.variableName)), tags.definition(tags.className)], fontWeight: "600" },
  { tag: tags.invalid, color: "var(--danger)" },
]);

const selection = "color-mix(in srgb, var(--accent) 28%, transparent)";

const theme = EditorView.theme({
  "&": { height: "100%", color: "var(--text)", backgroundColor: "var(--surface)", fontSize: "var(--text-sm)" },
  "&.cm-focused": { outline: "2px solid var(--accent)", outlineOffset: "-2px" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.6" },
  ".cm-content": { caretColor: "var(--text)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: selection,
  },
  ".cm-gutters": { color: "var(--text-muted)", backgroundColor: "var(--surface)", borderRight: "1px solid var(--border)" },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--surface-muted) 70%, transparent)" },
  ".cm-activeLineGutter": { color: "var(--text)", backgroundColor: "var(--surface-muted)" },
  "&.cm-focused .cm-matchingBracket": { backgroundColor: selection, outline: "none" },
});

/** What every document shares; also the empty view's state. */
export const baseExtensions: Extension = [
  lineNumbers(),
  highlightActiveLineGutter(),
  highlightActiveLine(),
  history(),
  drawSelection(),
  indentOnInput(),
  bracketMatching(),
  syntaxHighlighting(highlight),
  indentUnit.of("    "),
  EditorState.tabSize.of(4),
  // Tab indents; Escape then Tab leaves the editor (CodeMirror's tab-focus mode).
  keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
  theme,
];

export function documentExtensions(source: SourceDocument): Extension {
  return [
    baseExtensions,
    source.language === "python" ? pythonLanguage : [],
    EditorView.contentAttributes.of({ "aria-label": `Source of ${source.path}` }),
  ];
}
