import type { TimelineMark } from "../api/types.ts";

export type MarkKind = TimelineMark["kind"];

export interface Lane {
  kind: MarkKind;
  marks: TimelineMark[];
}

/** A previous-mark step from less than this far past a mark's start goes one mark further back. */
const STEP_BACK_SLACK_SECONDS = 0.25;
const MAX_SHUTTLE_RATE = 4;

/** One lane per kind present, beats above sections. */
export function lanes(marks: readonly TimelineMark[]): Lane[] {
  const kinds: MarkKind[] = ["beat", "section"];
  return kinds
    .map((kind) => ({ kind, marks: marks.filter((mark) => mark.kind === kind) }))
    .filter((lane) => lane.marks.length > 0);
}

/** The `kind` mark under `seconds`; where two touch, the later one. */
export function markAt(marks: readonly TimelineMark[], kind: MarkKind, seconds: number): TimelineMark | null {
  let found: TimelineMark | null = null;
  for (const mark of marks) {
    if (mark.kind === kind && mark.start_seconds <= seconds && seconds < mark.end_seconds) found = mark;
  }
  return found;
}

/** The start of the next (or previous) beat, or section when there are no beats; `null` when there is none. */
export function markStep(marks: readonly TimelineMark[], seconds: number, direction: 1 | -1): number | null {
  const beats = marks.filter((mark) => mark.kind === "beat");
  const starts = (beats.length > 0 ? beats : marks).map((mark) => mark.start_seconds).sort((a, b) => a - b);
  if (direction === 1) return starts.find((start) => start > seconds + 1e-3) ?? null;
  return starts.findLast((start) => start < seconds - STEP_BACK_SLACK_SECONDS) ?? null;
}

/** The frame showing at `seconds`. */
function frameIndex(seconds: number, fps: number): number {
  return Math.floor(seconds * fps + 1e-6);
}

/** Moves by whole frames and lands mid-frame, so decoder rounding never shows a neighbour. */
export function stepFrames(seconds: number, frames: number, fps: number, duration: number): number {
  const last = Math.max(0, Math.ceil(duration * fps) - 1);
  const index = Math.min(Math.max(frameIndex(seconds, fps) + frames, 0), last);
  return Math.min((index + 0.5) / fps, duration);
}

/**
 * J/K/L shuttle: K (0) stops; pressing the current direction again doubles
 * the speed up to 4×, the other direction starts over at 1×.
 */
export function shuttleRate(rate: number, direction: -1 | 0 | 1): number {
  if (direction === 0) return 0;
  if (Math.sign(rate) !== direction) return direction;
  return direction * Math.min(Math.abs(rate) * 2, MAX_SHUTTLE_RATE);
}
