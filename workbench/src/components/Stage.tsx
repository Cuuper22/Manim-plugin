import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  Expand,
  FileVideo2,
  Focus,
  Maximize,
  Pause,
  Play,
  Scan,
  SkipBack,
  SkipForward,
} from "lucide-react";
import type { BeatItem } from "../types";
import { IconButton } from "./Chrome";

interface StageProps {
  currentTime: number;
  duration: number;
  playing: boolean;
  fps: number;
  previewUrl?: string;
  demoMode: boolean;
  captions: BeatItem[];
  onPlayingChange: (playing: boolean) => void;
  onTimeChange: (time: number) => void;
}

export function formatTime(time: number, withFrames = false, fps = 30): string {
  const safe = Math.max(0, time);
  const hours = Math.floor(safe / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  const seconds = Math.floor(safe % 60);
  if (!withFrames) return `${hours ? `${String(hours).padStart(2, "0")}:` : ""}${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  const frames = Math.floor((safe % 1) * fps);
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${String(frames).padStart(2, "0")}`;
}

const sequence = [1, 1, 2.618, 5.236, 9.472, 16.708, 27.944, 46.652, 77.596];
const colors = ["#28b8f0", "#2ab5ef", "#37b6e9", "#50bde0", "#83c7d4", "#eeb2a4", "#ff937b", "#ff785f", "#ff6153"];

function MathVisualization({ currentTime, duration }: { currentTime: number; duration: number }) {
  const points = useMemo(() => sequence.map((value, index) => ({
    x: 122 + index * 94,
    y: 326 - Math.log2(value) * 31.5,
    value,
  })), []);
  const activeIndex = Math.min(8, Math.max(0, Math.floor((currentTime / Math.max(0.01, duration)) * 10)));
  const path = points.map((point, index) => `${index === 0 ? "M" : "L"}${point.x},${point.y}`).join(" ");

  return (
    <svg className="math-preview" viewBox="0 0 1000 520" role="img" aria-labelledby="math-title math-description">
      <title id="math-title">Generalized Fibonacci sequence visualization</title>
      <desc id="math-description">A weighted recurrence and its exponentially growing sequence plotted from zero through eight.</desc>
      <defs>
        <linearGradient id="curve-color" x1="0" x2="1">
          <stop offset="0" stopColor="#28b8f0" />
          <stop offset="0.55" stopColor="#f1b3a5" />
          <stop offset="1" stopColor="#ff6153" />
        </linearGradient>
        <filter id="point-glow" x="-200%" y="-200%" width="400%" height="400%">
          <feGaussianBlur stdDeviation="4" result="blur" />
          <feMerge><feMergeNode in="blur" /><feMergeNode in="SourceGraphic" /></feMerge>
        </filter>
      </defs>

      <text x="54" y="60" className="math-title">Generalized Fibonacci</text>
      <text x="54" y="111" className="math-formula">
        <tspan fontStyle="italic">u</tspan><tspan className="sub" baselineShift="sub" fontSize="16">n</tspan>
        <tspan> = </tspan><tspan fill="#2db9ef" fontStyle="italic">p</tspan>
        <tspan fontStyle="italic">u</tspan><tspan baselineShift="sub" fontSize="16">n−1</tspan>
        <tspan> + </tspan><tspan fill="#ff7d65" fontStyle="italic">q</tspan>
        <tspan fontStyle="italic">u</tspan><tspan baselineShift="sub" fontSize="16">n−2</tspan>
      </text>
      <text x="54" y="161" className="math-param" fill="#2db9ef"><tspan fontStyle="italic">p</tspan> = 1.618</text>
      <text x="54" y="201" className="math-param" fill="#ff7d65"><tspan fontStyle="italic">q</tspan> = 1.000</text>

      <line x1="122" y1="345" x2="890" y2="345" className="axis" />
      <path d="M 886 340 L 896 345 L 886 350" className="axis-arrow" />
      <text x="906" y="359" className="axis-name">n</text>
      <path d={path} fill="none" stroke="url(#curve-color)" strokeWidth="2.2" />
      {points.map((point, index) => (
        <g key={index} className={index === activeIndex ? "active-sequence-point" : ""}>
          <line x1={point.x} y1={point.y + 7} x2={point.x} y2="326" stroke={colors[index]} opacity="0.6" strokeDasharray="2 4" />
          <line x1={point.x} y1="339" x2={point.x} y2="351" className="tick" />
          <text x={point.x} y="372" textAnchor="middle" className="tick-label">{index}</text>
          <text x={point.x} y={point.y - 17} textAnchor="middle" className="value-label">{point.value}</text>
          <circle cx={point.x} cy={point.y} r={index === activeIndex ? 6.5 : 4.5} fill={colors[index]} filter={index === activeIndex ? "url(#point-glow)" : undefined} />
        </g>
      ))}

      <g transform="translate(54 376)" className="mini-axis">
        <line x1="0" y1="74" x2="72" y2="74" className="axis" />
        <path d="M68 70 L76 74 L68 78" className="axis-arrow" />
        <line x1="0" y1="74" x2="0" y2="10" className="axis" />
        <path d="M-4 16 L0 7 L4 16" className="axis-arrow" />
        <text x="-20" y="10" className="axis-name">uₙ</text>
        <text x="82" y="80" className="axis-name">n</text>
      </g>

      <g className="formula-card">
        <rect x="284" y="398" width="605" height="100" rx="4" />
        <text x="586" y="445" textAnchor="middle" className="closed-form">
          <tspan fontStyle="italic">uₙ</tspan><tspan> = </tspan><tspan fill="#59c5eb">1.618</tspan>
          <tspan fontStyle="italic"> uₙ₋₁</tspan><tspan> + </tspan><tspan fill="#ff7d65">1</tspan><tspan fontStyle="italic"> uₙ₋₂</tspan>
        </text>
        <text x="586" y="482" textAnchor="middle" className="sequence-formula">
          {'{uₙ} = {1, '}<tspan fill="#59c5eb">1, 2.618, 5.236</tspan><tspan>, </tspan><tspan fill="#ff7d65">9.472, 16.708</tspan><tspan>{', …}'}</tspan>
        </text>
      </g>
    </svg>
  );
}

export function Stage({ currentTime, duration, playing, fps, previewUrl, demoMode, captions, onPlayingChange, onTimeChange }: StageProps) {
  const [fit, setFit] = useState("Fit");
  const [zoom, setZoom] = useState("100%");
  const frameRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const activeCaption = captions.find((caption) => currentTime >= caption.start && currentTime < caption.end);
  const decodedPreviewUrl = previewUrl?.toLowerCase().replaceAll("%2e", ".") ?? "";
  const imagePreview = /\.(gif|png|jpe?g|webp|svg)(?:$|[?&#])/.test(decodedPreviewUrl);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (Math.abs(video.currentTime - currentTime) > 0.12) video.currentTime = currentTime;
  }, [currentTime]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (playing) void video.play().catch(() => onPlayingChange(false));
    else video.pause();
  }, [playing, onPlayingChange]);

  const jump = (direction: -1 | 1) => onTimeChange(direction < 0 ? 0 : duration);
  const step = (direction: -1 | 1) => onTimeChange(Math.max(0, Math.min(duration, currentTime + direction / fps)));

  return (
    <section className="stage" aria-label="Animation preview">
      <div className="stage-toolbar">
        <div className="playback-tools">
          <IconButton label="Go to start" onClick={() => jump(-1)}><SkipBack size={16} /></IconButton>
          <button type="button" className="play-button" aria-label={playing ? "Pause" : "Play"} onClick={() => onPlayingChange(!playing)}>
            {playing ? <Pause size={18} fill="currentColor" /> : <Play size={18} fill="currentColor" />}
          </button>
          <IconButton label="Previous frame" onClick={() => step(-1)}><SkipBack size={14} /></IconButton>
          <IconButton label="Next frame" onClick={() => step(1)}><SkipForward size={14} /></IconButton>
          <button type="button" className="time-readout" onClick={() => onTimeChange(0)} title="Reset playhead">
            <span>{formatTime(currentTime, true, fps)}</span><i>/</i>{formatTime(duration, true, fps)}
          </button>
        </div>
        <div className="view-tools">
          <label className="stage-select"><span className="sr-only">Stage fit</span><select value={fit} onChange={(event) => setFit(event.target.value)}><option>Fit</option><option>Fill</option><option>Actual</option></select></label>
          <label className="stage-select zoom"><span className="sr-only">Stage zoom</span><select value={zoom} onChange={(event) => setZoom(event.target.value)}><option>50%</option><option>75%</option><option>100%</option><option>125%</option><option>150%</option></select></label>
          <IconButton label="Focus selected object" onClick={() => setZoom("125%")}><Focus size={15} /></IconButton>
          <IconButton label="Frame all objects" onClick={() => { setFit("Fit"); setZoom("100%"); }}><Scan size={15} /></IconButton>
          <IconButton label="Enter fullscreen" onClick={() => void frameRef.current?.requestFullscreen()}><Maximize size={15} /></IconButton>
        </div>
      </div>
      <div className="stage-viewport" ref={frameRef} data-fit={fit.toLowerCase()} style={{ "--stage-zoom": `${Number.parseInt(zoom, 10) / 100}` } as CSSProperties}>
        {previewUrl && imagePreview ? <img className="rendered-preview" src={previewUrl} alt="Latest rendered preview" /> : previewUrl ? (
          <video
            ref={videoRef}
            className="rendered-preview"
            src={previewUrl}
            preload="metadata"
            onTimeUpdate={(event) => onTimeChange(event.currentTarget.currentTime)}
            onEnded={() => onPlayingChange(false)}
          />
        ) : demoMode ? <MathVisualization currentTime={currentTime} duration={duration} /> : (
          <div className="stage-empty">
            <FileVideo2 size={34} />
            <strong>No rendered preview</strong>
            <span>Render a declared scene to inspect its output here.</span>
          </div>
        )}
        {activeCaption ? <div className="preview-caption">{activeCaption.name}</div> : null}
        <div className="stage-resolution"><Expand size={12} /> 1280 × 720</div>
      </div>
    </section>
  );
}
