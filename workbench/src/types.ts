export type ConnectionMode = "connecting" | "online" | "demo";
export type Renderer = "Cairo" | "OpenGL";
export type QualityProfile = "Draft 480p" | "Preview 720p" | "Production 1080p" | "Cinema 4K";
export type LeftTab = "project" | "assets";
export type InspectorTab = "inspector" | "code";
export type BottomTab = "console" | "queue" | "exports";
export type ExportFormat = "bundle" | "mp4" | "webm" | "gif" | "captions";

export interface SceneItem {
  id: string;
  name: string;
  start: number;
  end: number;
  file: string;
  color: string;
  enabled?: boolean;
}

export interface BeatItem {
  id: string;
  sceneId: string;
  name: string;
  start: number;
  end: number;
  kind: "beat" | "visual" | "caption";
  color?: string;
  source?: SourceLocation;
}

export interface SourceLocation {
  file: string;
  line: number;
  object: string;
}

export interface SelectedObject {
  id: string;
  name: string;
  type: string;
  visible: boolean;
  locked: boolean;
  position: [number, number, number];
  scale: [number, number, number];
  rotation: number;
  anchor: string;
  color: string;
  fillOpacity: number;
  stroke: string;
  strokeWidth: number;
  appear: number;
  start: number;
  end: number;
  fadeIn: number;
  fadeOut: number;
  easing: string;
  source: SourceLocation;
}

export interface AssetItem {
  id: string;
  name: string;
  type: "image" | "svg" | "audio" | "video" | "font";
  size: string;
  path: string;
}

export interface LogEntry {
  id: string;
  time: string;
  level: "INFO" | "WARNING" | "ERROR";
  message: string;
  source: string;
}

export interface RenderJob {
  id: string;
  scene: string;
  profile: string;
  renderer: string;
  status: "queued" | "rendering" | "complete" | "failed" | "cancelled";
  progress: number;
  frames: number;
  totalFrames: number;
  output?: string;
  operation?: string;
  request?: Record<string, unknown>;
}

export interface ExportArtifact {
  id: string;
  name: string;
  format: string;
  size: string;
  createdAt: string;
  url?: string;
}

export interface CameraPoint {
  time: number;
  value: number;
}

export interface WorkspaceState {
  projectId: string;
  projectName: string;
  duration: number;
  fps: number;
  previewUrl?: string;
  latestArtifactPath?: string;
  captionSourcePath?: string;
  scenes: SceneItem[];
  beats: BeatItem[];
  assets: AssetItem[];
  selection: SelectedObject;
  sourceCode: string;
  logs: LogEntry[];
  renderQueue: RenderJob[];
  exports: ExportArtifact[];
  camera: CameraPoint[];
  waveform: number[];
}

export interface DirectorEvent {
  type: "job_queued" | "job_started" | "job_progress" | "job_finished";
  job?: unknown;
  job_id?: string;
  event?: string;
  data?: unknown;
  result?: unknown;
  error?: unknown;
}

export interface ActionResult<T = unknown> {
  ok: boolean;
  data?: T;
  message?: string;
}
