import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

/** Every `--name: light-dark(#light, #dark)` color token of tokens.css. */
function themes(): { light: Map<string, string>; dark: Map<string, string> } {
  const css = readFileSync(new URL("../src/tokens.css", import.meta.url), "utf8");
  const light = new Map<string, string>();
  const dark = new Map<string, string>();
  for (const [, name, day, night] of css.matchAll(/--([\w-]+):\s*light-dark\((#[0-9a-f]{6}),\s*(#[0-9a-f]{6})\)/g)) {
    light.set(name!, day!);
    dark.set(name!, night!);
  }
  return { light, dark };
}

function luminance(hex: string): number {
  const channel = (offset: number) => {
    const value = Number.parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

function contrast(a: string, b: string): number {
  const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (high! + 0.05) / (low! + 0.05);
}

/** Text colors and the surfaces they are drawn on. */
const PAIRS: Record<string, readonly string[]> = {
  text: ["bg", "surface", "surface-muted", "stage"],
  "text-muted": ["bg", "surface", "surface-muted", "stage"],
  accent: ["surface", "surface-muted"],
  "on-accent": ["accent"],
  danger: ["surface", "surface-muted"],
  warning: ["surface", "surface-muted"],
  success: ["surface", "surface-muted"],
};

test("text tokens keep 4.5:1 contrast on their surfaces in both themes", () => {
  for (const [theme, tokens] of Object.entries(themes())) {
    for (const [foreground, backgrounds] of Object.entries(PAIRS)) {
      for (const background of backgrounds) {
        const ratio = contrast(tokens.get(foreground)!, tokens.get(background)!);
        assert.ok(ratio >= 4.5, `${theme}: ${foreground} on ${background} is ${ratio.toFixed(2)}:1`);
      }
    }
  }
});

test("colors are spelled out only in tokens.css", () => {
  const styles = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.deepEqual(styles.match(/#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/gi), null);
});
