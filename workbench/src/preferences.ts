// Per-browser conveniences only; storage may be unavailable (private windows), so every access is guarded.

const PREFIX = "manim-director.";

export function readPreference(key: string): string | null {
  try {
    return localStorage.getItem(PREFIX + key);
  } catch {
    return null;
  }
}

export function writePreference(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(PREFIX + key);
    else localStorage.setItem(PREFIX + key, value);
  } catch {
    // Not remembered; nothing else depends on it.
  }
}
