import { useRef, useState } from "react";
import {
  Captions,
  ChevronDown,
  Eye,
  Hand,
  Link2,
  MousePointer2,
  Scissors,
  Send,
  Sparkles,
  Volume2,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import type { BeatItem, CameraPoint, SceneItem } from "../types";
import { IconButton } from "./Chrome";

interface TimelineProps {
  currentTime: number;
  duration: number;
  scenes: SceneItem[];
  beats: BeatItem[];
  waveform: number[];
  camera: CameraPoint[];
  zoom: number;
  selectedBeatId?: string;
  onZoomChange: (zoom: number) => void;
  onSeek: (time: number) => void;
  onBeatSelect: (beat: BeatItem) => void;
  onRunIntent: (command: string) => Promise<void> | void;
}

const labelWidth = 154;

function TrackLabel({ icon, name, disclosure = false }: { icon?: React.ReactNode; name: string; disclosure?: boolean }) {
  return (
    <div className="track-label">
      {disclosure ? <ChevronDown size={13} /> : icon ?? <Eye size={13} />}
      <span>{name}</span>
      {name === "Narration" ? <Volume2 size={13} className="track-label-tail" /> : null}
    </div>
  );
}

function clipStyle(start: number, end: number, pixelsPerSecond: number): React.CSSProperties {
  return {
    left: start * pixelsPerSecond,
    width: Math.max(4, (end - start) * pixelsPerSecond - 2),
  };
}

export function Timeline({
  currentTime,
  duration,
  scenes,
  beats,
  waveform,
  camera,
  zoom,
  selectedBeatId,
  onZoomChange,
  onSeek,
  onBeatSelect,
  onRunIntent,
}: TimelineProps) {
  const [command, setCommand] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [tool, setTool] = useState<"select" | "pan" | "split" | "link">("select");
  const scrollRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const pixelsPerSecond = 22 * zoom;
  const trackWidth = Math.max(840, duration * pixelsPerSecond + 30);
  const timelineWidth = trackWidth + labelWidth;
  const visualBeats = beats.filter((beat) => beat.kind === "visual");
  const storyBeats = beats.filter((beat) => beat.kind === "beat");
  const captionBeats = beats.filter((beat) => beat.kind === "caption");
  const ticks = Array.from({ length: Math.floor(duration / 5) + 1 }, (_, index) => index * 5);

  const seekFromPointer = (clientX: number) => {
    const viewport = scrollRef.current;
    if (!viewport) return;
    const rect = viewport.getBoundingClientRect();
    const relative = clientX - rect.left + viewport.scrollLeft - labelWidth;
    onSeek(Math.max(0, Math.min(duration, relative / pixelsPerSecond)));
  };

  const submit = async () => {
    if (!command.trim() || submitting) return;
    setSubmitting(true);
    try {
      await onRunIntent(command.trim());
      setCommand("");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <section className="timeline" aria-label="Animation timeline">
      <div className="timeline-toolbar">
        <div className="edit-tools">
          <IconButton label="Selection tool" active={tool === "select"} onClick={() => setTool("select")}><MousePointer2 size={16} /></IconButton>
          <IconButton label="Pan timeline" active={tool === "pan"} onClick={() => setTool("pan")}><Hand size={16} /></IconButton>
          <IconButton label="Split clip" active={tool === "split"} onClick={() => setTool("split")}><Scissors size={16} /></IconButton>
          <IconButton label="Link clips" active={tool === "link"} onClick={() => setTool("link")}><Link2 size={16} /></IconButton>
        </div>
        <form className="command-bar" onSubmit={(event) => { event.preventDefault(); void submit(); }}>
          <Sparkles size={15} />
          <input
            aria-label="Direct the animation"
            placeholder="Run preview, QA, export, or inspect…"
            value={command}
            onChange={(event) => setCommand(event.target.value)}
            data-command-input
          />
          <button type="submit" aria-label="Run direction" title="Run direction" disabled={!command.trim() || submitting}>
            <Send size={14} />
          </button>
        </form>
        <div className="timeline-zoom">
          <IconButton label="Zoom out" onClick={() => onZoomChange(Math.max(0.55, zoom - 0.15))}><ZoomOut size={14} /></IconButton>
          <input
            aria-label="Timeline zoom"
            type="range"
            min="0.55"
            max="2.4"
            step="0.05"
            value={zoom}
            onChange={(event) => onZoomChange(Number(event.target.value))}
          />
          <IconButton label="Zoom in" onClick={() => onZoomChange(Math.min(2.4, zoom + 0.15))}><ZoomIn size={14} /></IconButton>
        </div>
      </div>

      <div
        ref={scrollRef}
        className="timeline-scroll"
        onPointerDown={(event) => {
          if ((event.target as HTMLElement).closest("button, input")) return;
          dragging.current = true;
          event.currentTarget.setPointerCapture(event.pointerId);
          seekFromPointer(event.clientX);
        }}
        onPointerMove={(event) => { if (dragging.current) seekFromPointer(event.clientX); }}
        onPointerUp={(event) => {
          dragging.current = false;
          if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
        }}
        onPointerCancel={() => { dragging.current = false; }}
      >
        <div className="timeline-inner" style={{ width: timelineWidth }}>
          <div className="time-ruler timeline-row">
            <TrackLabel name="" />
            <div className="track-surface ruler-surface" style={{ width: trackWidth }}>
              {ticks.map((tick) => (
                <div className="ruler-tick" key={tick} style={{ left: tick * pixelsPerSecond }}>
                  <span>{Math.floor(tick / 60)}:{String(tick % 60).padStart(2, "0")}</span>
                </div>
              ))}
            </div>
          </div>

          <div className="timeline-row scene-track">
            <TrackLabel name="Scenes" />
            <div className="track-surface" style={{ width: trackWidth }}>
              {scenes.map((scene) => (
                <button
                  type="button"
                  className="scene-clip"
                  style={{ ...clipStyle(scene.start, scene.end, pixelsPerSecond), "--clip-color": scene.color } as React.CSSProperties}
                  key={scene.id}
                  onClick={(event) => { event.stopPropagation(); onSeek(scene.start); }}
                  title={`${scene.name} · ${scene.end - scene.start}s`}
                >{scene.name}</button>
              ))}
              {scenes.length === 0 ? <span className="track-empty">No declared scenes</span> : null}
            </div>
          </div>

          <div className="timeline-row beat-track">
            <TrackLabel name="Beats" />
            <div className="track-surface" style={{ width: trackWidth }}>
              {storyBeats.map((beat) => (
                <button
                  type="button"
                  key={beat.id}
                  className={`beat-marker ${selectedBeatId === beat.id ? "selected" : ""}`}
                  style={{ left: beat.start * pixelsPerSecond }}
                  onClick={(event) => { event.stopPropagation(); onBeatSelect(beat); }}
                >
                  <i />
                  <span>{beat.name}</span>
                </button>
              ))}
              {storyBeats.length === 0 ? <span className="track-empty">No storyboard beats</span> : null}
            </div>
          </div>

          <div className="timeline-row visual-track">
            <TrackLabel name="Visuals" />
            <div className="track-surface" style={{ width: trackWidth }}>
              {visualBeats.map((beat) => (
                <button
                  type="button"
                  key={beat.id}
                  className={`visual-clip ${selectedBeatId === beat.id ? "selected" : ""}`}
                  style={clipStyle(beat.start, beat.end, pixelsPerSecond)}
                  onClick={(event) => { event.stopPropagation(); onBeatSelect(beat); }}
                >{beat.name}</button>
              ))}
            </div>
          </div>

          <div className="timeline-row narration-track">
            <TrackLabel name="Narration" />
            <div className="track-surface waveform" style={{ width: trackWidth }}>
              {waveform.map((height, index) => (
                <i key={index} style={{ left: `${(index / waveform.length) * 100}%`, height: `${height * 88}%` }} />
              ))}
            </div>
          </div>

          <div className="timeline-row caption-track">
            <TrackLabel name="Captions" icon={<Captions size={13} />} />
            <div className="track-surface" style={{ width: trackWidth }}>
              {captionBeats.map((beat) => (
                <button
                  type="button"
                  key={beat.id}
                  className="caption-clip"
                  style={clipStyle(beat.start, beat.end, pixelsPerSecond)}
                  onClick={(event) => { event.stopPropagation(); onBeatSelect(beat); }}
                  title={beat.name}
                >{beat.name}</button>
              ))}
            </div>
          </div>

          <div className="timeline-row camera-track">
            <TrackLabel name="Camera" disclosure />
            <div className="track-surface camera-curve" style={{ width: trackWidth }}>
              <svg width={trackWidth} height="34" preserveAspectRatio="none" aria-label="Camera animation curve">
                <polyline
                  points={camera.map((point) => `${point.time * pixelsPerSecond},${30 - point.value * 24}`).join(" ")}
                  fill="none"
                  stroke="#20a8ed"
                  strokeWidth="1.2"
                />
                {camera.map((point, index) => (
                  <rect key={index} x={point.time * pixelsPerSecond - 2.5} y={30 - point.value * 24 - 2.5} width="5" height="5" transform={`rotate(45 ${point.time * pixelsPerSecond} ${30 - point.value * 24})`} fill="#20a8ed" />
                ))}
              </svg>
            </div>
          </div>

          <div className="playhead" style={{ left: labelWidth + currentTime * pixelsPerSecond }} aria-hidden="true">
            <span>{currentTime.toFixed(2)}</span><i />
          </div>
        </div>
      </div>
    </section>
  );
}
