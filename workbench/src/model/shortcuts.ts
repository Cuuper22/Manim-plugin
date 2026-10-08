export type Command =
  | "toggle_play"
  | "frame_back"
  | "frame_forward"
  | "beat_back"
  | "beat_forward"
  | "reverse"
  | "stop"
  | "forward"
  | "save"
  | "preview"
  | "close_overlay"
  | "show_shortcuts";

export interface KeyInput {
  key: string;
  shiftKey: boolean;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}

/**
 * Where focus is: the code editor; a text field or select; a button (owns
 * Space); a tab, menu item or slider (owns Space and the arrows); or nothing
 * interactive.
 */
export type FocusZone = "editor" | "text" | "button" | "composite" | "page";

/** The command a key press runs, if any. Cmd and Ctrl are interchangeable. */
export function commandFor(input: KeyInput, zone: FocusZone): Command | null {
  const { key } = input;
  if ((input.metaKey || input.ctrlKey) && !input.altKey) {
    if (key === "s" || key === "S") return "save";
    if (key === "Enter") return "preview";
    return null;
  }
  if (input.metaKey || input.ctrlKey || input.altKey) return null;
  if (key === "Escape") return zone === "editor" ? null : "close_overlay";
  if (zone === "editor" || zone === "text") return null;

  switch (key) {
    case " ":
      return zone === "page" ? "toggle_play" : null;
    case "ArrowLeft":
      if (zone === "composite") return null;
      return input.shiftKey ? "beat_back" : "frame_back";
    case "ArrowRight":
      if (zone === "composite") return null;
      return input.shiftKey ? "beat_forward" : "frame_forward";
    case "j":
    case "J":
      return "reverse";
    case "k":
    case "K":
      return "stop";
    case "l":
    case "L":
      return "forward";
    case "?":
      return "show_shortcuts";
    default:
      return null;
  }
}

/** The reference the shortcuts dialog shows; `Mod` reads Cmd on Apple platforms, else Ctrl. */
export const SHORTCUTS: readonly { keys: readonly string[]; action: string }[] = [
  { keys: ["Space"], action: "Play or pause" },
  { keys: ["←", "→"], action: "Step one frame" },
  { keys: ["Shift ←", "Shift →"], action: "Previous or next beat" },
  { keys: ["J", "K", "L"], action: "Play backwards, stop, play forwards (repeat to speed up)" },
  { keys: ["Mod S"], action: "Save the open file" },
  { keys: ["Mod Enter"], action: "Save, then preview the scene" },
  { keys: ["Esc"], action: "Close a menu or dialog (outside the editor)" },
  { keys: ["?"], action: "Show these shortcuts" },
];
