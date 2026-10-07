import { useMemo, useSyncExternalStore, type KeyboardEvent, type PointerEvent } from "react";
import { formatTime } from "../model/format.ts";
import { lanes, markAt, rulerTicks, tickLabel } from "../model/timeline.ts";
import type { PlaybackController } from "../stage/playback.ts";

const percent = (part: number, whole: number) => `${whole > 0 ? (part / whole) * 100 : 0}%`;

/** Transport and a scrubbable track of the render's beats and sections; the track is a slider. */
export function Timeline({ playback }: { playback: PlaybackController }) {
  const { source, time, rate } = useSyncExternalStore(playback.subscribe, playback.getSnapshot);
  const ticks = useMemo(() => rulerTicks(source?.duration ?? 0), [source]);
  if (!source) return null;
  const { duration, marks } = source;
  const current = markAt(marks, "beat", time) ?? markAt(marks, "section", time);

  const seekTo = (event: PointerEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    playback.seek(((event.clientX - box.left) / box.width) * duration);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const keys: Record<string, () => void> = {
      ArrowLeft: () => (event.shiftKey ? playback.stepMark(-1) : playback.stepFrames(-1)),
      ArrowRight: () => (event.shiftKey ? playback.stepMark(1) : playback.stepFrames(1)),
      ArrowDown: () => playback.stepFrames(-1),
      ArrowUp: () => playback.stepFrames(1),
      PageDown: () => playback.stepMark(-1),
      PageUp: () => playback.stepMark(1),
      Home: () => playback.seek(0),
      End: () => playback.seek(duration),
      " ": () => playback.toggle(),
    };
    const run = keys[event.key];
    if (!run || event.metaKey || event.ctrlKey || event.altKey) return;
    event.preventDefault();
    run();
  };

  return (
    <div className="timeline">
      <button
        type="button"
        className="transport"
        aria-label={rate === 0 ? "Play" : "Pause"}
        aria-keyshortcuts="Space"
        onClick={() => playback.toggle()}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          {rate === 0 ? <path d="M4 2.5v11l9-5.5z" /> : <path d="M4 2.5h3v11H4zM9 2.5h3v11H9z" />}
        </svg>
      </button>
      <span className="mono timecode">
        {formatTime(time)}
        <span className="muted"> / {formatTime(duration)}</span>
      </span>
      <div
        className="track"
        role="slider"
        tabIndex={0}
        aria-label="Playhead"
        aria-valuemin={0}
        aria-valuemax={Number(duration.toFixed(2))}
        aria-valuenow={Number(time.toFixed(2))}
        aria-valuetext={`${formatTime(time)}${current ? `, ${current.kind} ${current.name}` : ""}`}
        onKeyDown={onKeyDown}
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId);
          seekTo(event);
        }}
        onPointerMove={(event) => event.currentTarget.hasPointerCapture(event.pointerId) && seekTo(event)}
      >
        <div className="ruler" aria-hidden="true">
          {ticks.map(({ seconds, labelled }) => (
            <span key={seconds} className="tick" data-labelled={labelled || undefined} style={{ left: percent(seconds, duration) }}>
              {labelled ? <span>{tickLabel(seconds)}</span> : null}
            </span>
          ))}
        </div>
        {lanes(marks).map((lane) => (
          <div key={lane.kind} className="lane" data-kind={lane.kind}>
            {lane.marks.map((mark) => (
              <span
                key={`${mark.name}:${mark.start_seconds}`}
                className="mark"
                data-current={mark === current || undefined}
                style={{
                  left: percent(mark.start_seconds, duration),
                  width: percent(mark.end_seconds - mark.start_seconds, duration),
                }}
                title={`${mark.name} · ${formatTime(mark.start_seconds)}–${formatTime(mark.end_seconds)}`}
              >
                {mark.name}
              </span>
            ))}
          </div>
        ))}
        <span className="playhead" style={{ left: percent(time, duration) }} />
      </div>
      <span className="meta mono" aria-live="polite">
        {rate !== 0 && rate !== 1 ? `${rate < 0 ? "−" : ""}${Math.abs(rate)}×` : ""}
      </span>
    </div>
  );
}
