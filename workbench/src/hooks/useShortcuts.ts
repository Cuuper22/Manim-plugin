import { useEffect, useRef, type RefObject } from "react";
import { commandFor, type Command, type FocusZone } from "../model/shortcuts.ts";

const COMPOSITE_ROLES = new Set(["tab", "slider", "menuitem", "radio", "option"]);

/** Which keys the focused element needs for itself. */
export function focusZone(target: EventTarget | null): FocusZone {
  if (!(target instanceof Element)) return "page";
  if (target.closest(".cm-editor")) return "editor";
  // A modal dialog keeps the stage's playback keys away from what is behind it.
  if (target.matches("input, textarea, select, [contenteditable='true']") || target.closest("dialog[open]")) return "text";
  if (COMPOSITE_ROLES.has(target.getAttribute("role") ?? "")) return "composite";
  if (target.matches("button, a[href], summary")) return "button";
  return "page";
}

export type CommandHandlers = Partial<Record<Command, () => void>>;

/** Runs the workbench's keyboard commands; components that handle a key themselves call `preventDefault`. */
export function useShortcuts(handlers: CommandHandlers): void {
  const current = useRef(handlers);
  current.current = handlers;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      const command = commandFor(event, focusZone(event.target));
      const run = command ? current.current[command] : undefined;
      if (!run || (event.repeat && command === "toggle_play")) return;
      event.preventDefault();
      run();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
}

/** Closes a popover on Escape (unless typing in the editor) and on a press outside `within`. */
export function useDismiss(open: boolean, close: () => void, within: RefObject<HTMLElement | null>): void {
  const current = useRef(close);
  current.current = close;
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (commandFor(event, focusZone(event.target)) === "close_overlay") current.current();
    };
    const onPointer = (event: PointerEvent) => {
      if (!within.current?.contains(event.target as Node)) current.current();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onPointer);
    };
  }, [open, within]);
}
