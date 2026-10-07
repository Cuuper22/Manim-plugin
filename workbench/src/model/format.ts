/** `m:ss.cc`, e.g. `0:03.20`; negative and non-finite input reads as zero. */
export function formatTime(seconds: number): string {
  const centis = Number.isFinite(seconds) && seconds > 0 ? Math.round(seconds * 100) : 0;
  const minutes = Math.floor(centis / 6000);
  const rest = centis - minutes * 6000;
  const whole = Math.floor(rest / 100);
  return `${minutes}:${String(whole).padStart(2, "0")}.${String(rest % 100).padStart(2, "0")}`;
}

/** `1.5 s`, with at most two decimals. */
export function formatSeconds(seconds: number): string {
  return `${Number(seconds.toFixed(2))} s`;
}

/** The last segment of a POSIX path. */
export function baseName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}
