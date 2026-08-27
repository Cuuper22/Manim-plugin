import type {
  BeatItem,
  DirectorEvent,
  ExportFormat,
  LogEntry,
  QualityProfile,
  RenderJob,
  Renderer,
  SceneItem,
  SelectedObject,
  WorkspaceState,
} from "../types";
import {
  loadCompleteSource,
  MAX_SOURCE_BYTES,
  sourceByteLength,
  type CompleteSourceDocument,
  type SourcePage,
} from "./sourcePaging";

const DEFAULT_TIMEOUT = 4_000;
const SCENE_COLORS = ["#298bd0", "#16a49b", "#2cad69", "#7852a3"];
type JsonMap = Record<string, unknown>;

function configuredBaseUrl(): string {
  const configured = import.meta.env.VITE_DIRECTOR_API_URL as string | undefined;
  return configured?.replace(/\/$/, "") ?? "";
}

async function requestJson<T>(path: string, init: RequestInit = {}, timeout = DEFAULT_TIMEOUT): Promise<T> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), timeout);
  try {
    const response = await fetch(`${configuredBaseUrl()}${path}`, {
      ...init,
      signal: controller.signal,
      headers: {
        Accept: "application/json",
        ...(init.body ? { "Content-Type": "application/json" } : {}),
        ...init.headers,
      },
    });
    if (!response.ok) {
      const detail = await response.text().catch(() => "");
      throw new Error(detail || `${response.status} ${response.statusText}`);
    }
    if (response.status === 204) return undefined as T;
    return (await response.json()) as T;
  } finally {
    window.clearTimeout(timer);
  }
}

export interface ApiJob {
  id: string;
  project_root?: string;
  operation: string;
  status: "queued" | "running" | "succeeded" | "failed" | "cancelled";
  params?: JsonMap;
  result?: JsonMap | null;
  error?: { message?: string } | null;
}

interface ApiStoryboardBeat {
  id?: string;
  objective?: string;
  visual?: string;
  duration?: number;
  duration_seconds?: number;
  narration_cue?: string;
}

interface ApiSceneSpec {
  id?: string;
  class?: string;
  class_name?: string;
  file?: string;
  purpose?: string;
  duration_seconds?: number;
}

interface ApiTimelineScene {
  id: string;
  name?: string;
  class_name?: string;
  file?: string;
  start_seconds: number;
  end_seconds: number;
  color?: string;
}

interface ApiTimelineBeat {
  id: string;
  scene_id?: string;
  name: string;
  kind?: "beat" | "visual" | "caption";
  start_seconds: number;
  end_seconds: number;
  color?: string;
  source?: { file: string; line: number; object: string };
}

interface ApiState {
  project_root: string;
  preview_url?: string;
  scenes?: ApiSceneSpec[];
  storyboard?: ApiStoryboardBeat[];
  duration_seconds?: number;
  latest_artifact?: { path: string; download_url: string; job_id: string } | null;
  timeline?: {
    duration_seconds?: number;
    preview_url?: string;
    scenes?: ApiTimelineScene[];
    beats?: ApiTimelineBeat[];
    waveform?: number[];
    camera?: Array<{ time: number; value: number }>;
  };
  spec: {
    project: { name: string; id?: string; title?: string };
    brief?: { duration_seconds?: number };
    engine?: { source?: string; main_scene?: string };
    render: { fps: number; renderer: string; profile: string };
    captions?: { source?: string };
    storyboard?: ApiStoryboardBeat[];
    scenes?: ApiSceneSpec[];
  };
  files: { sources: string[]; assets: string[]; outputs?: string[] };
  jobs: { items: ApiJob[] };
}

interface ApiLog {
  cursor: number;
  timestamp: string;
  level: string;
  event: string;
  data: JsonMap;
}

function titleFromPath(path: string): string {
  const leaf = path.split(/[\\/]/).at(-1)?.replace(/\.[^.]+$/, "") ?? "Scene";
  return leaf.replace(/[_-]+/g, " ").replace(/\b\w/g, (character) => character.toUpperCase());
}

function asFinite(value: unknown, fallback = 0): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function artifactPath(job: ApiJob): string | undefined {
  const result = job.result ?? {};
  const artifacts = Array.isArray(result.artifacts) ? result.artifacts : [];
  const raw = typeof result.output === "string"
    ? result.output
    : typeof result.path === "string"
      ? result.path
      : typeof artifacts[0] === "string"
        ? artifacts[0]
        : undefined;
  if (!raw) return undefined;
  const normalized = raw.replaceAll("\\", "/");
  const root = job.project_root?.replaceAll("\\", "/").replace(/\/$/, "");
  return root && normalized.startsWith(`${root}/`) ? normalized.slice(root.length + 1) : normalized;
}

export function artifactUrl(path: string): string {
  return `${configuredBaseUrl()}/api/files?path=${encodeURIComponent(path)}`;
}

export function jobToRenderJob(job: ApiJob, progressData?: unknown): RenderJob {
  const params = job.params ?? {};
  const data = progressData && typeof progressData === "object" ? progressData as JsonMap : {};
  const result = job.result ?? {};
  const totalFrames = asFinite(data.total_frames ?? result.total_frames ?? result.frames, 1);
  const frames = asFinite(data.frame ?? data.frames ?? (job.status === "succeeded" ? totalFrames : 0));
  const rawProgress = asFinite(data.progress, totalFrames > 0 ? (frames / totalFrames) * 100 : 0);
  const status: RenderJob["status"] = job.status === "running" ? "rendering" : job.status === "succeeded" ? "complete" : job.status;
  return {
    id: job.id,
    scene: String(params.scene ?? titleFromPath(String(params.scene_file ?? job.operation))),
    profile: String(params.profile ?? "Director"),
    renderer: String(params.renderer ?? "Cairo"),
    status,
    progress: Math.max(0, Math.min(100, Math.round(rawProgress))),
    frames: Math.max(0, Math.round(frames)),
    totalFrames: Math.max(1, Math.round(totalFrames)),
    output: artifactPath(job),
    operation: job.operation,
    request: params,
  };
}

function normalizeLog(log: ApiLog): LogEntry {
  const detail = typeof log.data.message === "string" ? log.data.message : typeof log.data.line === "string" ? log.data.line : JSON.stringify(log.data);
  return {
    id: String(log.cursor),
    time: new Date(log.timestamp).toLocaleTimeString("en-US", { hour12: false }),
    level: log.level.toUpperCase() === "WARN" ? "WARNING" : log.level.toUpperCase() === "ERROR" ? "ERROR" : "INFO",
    message: detail === "{}" ? log.event : `${log.event}: ${detail}`,
    source: "director.engine",
  };
}

function neutralSelection(sourceFile = "", sceneName = "No selection", accent = "#159fe8"): SelectedObject {
  return {
    id: "no-selection",
    name: sceneName,
    type: sourceFile ? "Scene" : "—",
    visible: true,
    locked: false,
    position: [0, 0, 0],
    scale: [1, 1, 1],
    rotation: 0,
    anchor: "CENTER",
    color: accent,
    fillOpacity: 1,
    stroke: accent,
    strokeWidth: 1,
    appear: 0,
    start: 0,
    end: 0,
    fadeIn: 0,
    fadeOut: 0,
    easing: "smooth",
    source: { file: sourceFile, line: 1, object: sceneName },
  };
}

function buildScenes(raw: ApiState, storyboardDuration: number): SceneItem[] {
  if (raw.timeline?.scenes?.length) {
    return raw.timeline.scenes.map((scene, index) => ({
      id: scene.id,
      name: scene.name ?? scene.class_name ?? scene.id,
      start: asFinite(scene.start_seconds),
      end: asFinite(scene.end_seconds),
      file: scene.file ?? raw.spec.engine?.source ?? "",
      color: scene.color ?? SCENE_COLORS[index % SCENE_COLORS.length],
      enabled: true,
    }));
  }

  const declaredScenes = raw.scenes?.length ? raw.scenes : raw.spec.scenes ?? [];
  const declaredStoryboard = raw.storyboard?.length ? raw.storyboard : raw.spec.storyboard ?? [];
  if (declaredScenes.length) {
    let cursor = 0;
    return declaredScenes.map((scene, index) => {
      const matchingBeat = declaredStoryboard.find((beat) => beat.id && beat.id === scene.id);
      const duration = asFinite(scene.duration_seconds ?? matchingBeat?.duration ?? matchingBeat?.duration_seconds);
      const item = {
        id: scene.id || `scene-${index + 1}`,
        name: scene.class ?? scene.class_name ?? scene.id ?? titleFromPath(scene.file ?? `scene-${index + 1}`),
        start: cursor,
        end: cursor + duration,
        file: scene.file ?? raw.spec.engine?.source ?? raw.files.sources[0] ?? "",
        color: SCENE_COLORS[index % SCENE_COLORS.length],
        enabled: true,
      };
      cursor += duration;
      return item;
    });
  }

  const mainScene = raw.spec.engine?.main_scene;
  if (!mainScene) return [];
  return [{
    id: mainScene,
    name: mainScene,
    start: 0,
    end: storyboardDuration,
    file: raw.spec.engine?.source ?? raw.files.sources[0] ?? "",
    color: SCENE_COLORS[0],
    enabled: true,
  }];
}

function buildBeats(raw: ApiState, scenes: SceneItem[]): BeatItem[] {
  if (raw.timeline?.beats?.length) {
    return raw.timeline.beats.map((beat) => ({
      id: beat.id,
      sceneId: beat.scene_id ?? scenes[0]?.id ?? "",
      name: beat.name,
      start: asFinite(beat.start_seconds),
      end: asFinite(beat.end_seconds),
      kind: beat.kind ?? "beat",
      color: beat.color,
      source: beat.source,
    }));
  }

  let cursor = 0;
  const declaredStoryboard = raw.storyboard?.length ? raw.storyboard : raw.spec.storyboard ?? [];
  return declaredStoryboard.flatMap((beat, index) => {
    const duration = asFinite(beat.duration ?? beat.duration_seconds);
    const start = cursor;
    const end = start + duration;
    cursor = end;
    const id = beat.id || `beat-${index + 1}`;
    const scene = scenes.find((item) => item.id === id) ?? scenes[0];
    const source = scene?.file ? { file: scene.file, line: 1, object: scene.name } : undefined;
    const items: BeatItem[] = [{
      id,
      sceneId: scene?.id ?? "",
      name: beat.objective ?? titleFromPath(id),
      start,
      end,
      kind: "beat",
      source,
    }];
    if (beat.visual) {
      items.push({ id: `${id}-visual`, sceneId: scene?.id ?? "", name: beat.visual, start, end, kind: "visual", source });
    }
    return items;
  });
}

function normalizeState(raw: ApiState, logs: LogEntry[]): WorkspaceState {
  const declaredStoryboard = raw.storyboard?.length ? raw.storyboard : raw.spec.storyboard ?? [];
  const storyboardDuration = declaredStoryboard.reduce((total, beat) => total + asFinite(beat.duration ?? beat.duration_seconds), 0);
  const scenes = buildScenes(raw, storyboardDuration);
  const beats = buildBeats(raw, scenes);
  const queue = raw.jobs.items.map((job) => jobToRenderJob(job));
  const inferredDuration = Math.max(0, ...scenes.map((scene) => scene.end), ...beats.map((beat) => beat.end));
  const duration = asFinite(raw.duration_seconds ?? raw.timeline?.duration_seconds ?? raw.spec.brief?.duration_seconds, inferredDuration || storyboardDuration);
  const assets: WorkspaceState["assets"] = raw.files.assets.map((path, index) => {
    const extension = path.split(".").at(-1)?.toLowerCase();
    const type: WorkspaceState["assets"][number]["type"] = extension === "svg" ? "svg" : ["wav", "mp3", "flac", "ogg"].includes(extension ?? "") ? "audio" : ["mp4", "mov", "webm"].includes(extension ?? "") ? "video" : ["otf", "ttf", "woff", "woff2"].includes(extension ?? "") ? "font" : "image";
    return { id: `asset-${index}`, name: path.split(/[\\/]/).at(-1) ?? path, type, size: "on disk", path };
  });
  const artifacts = queue.filter((job) => job.status === "complete" && job.output).map((job) => ({
    id: `artifact-${job.id}`,
    name: job.output!.split(/[\\/]/).at(-1) ?? job.output!,
    format: job.output!.split(".").at(-1)?.toUpperCase() ?? "FILE",
    size: "on disk",
    createdAt: "complete",
    url: artifactUrl(job.output!),
  }));
  const latestVideo = queue.find((job) => job.status === "complete" && job.output && /\.(mp4|mov|webm)$/i.test(job.output));
  const latestArtifactPath = raw.latest_artifact?.path ?? latestVideo?.output;
  const explicitPreview = raw.preview_url ?? raw.timeline?.preview_url;
  const artifactPreview = latestArtifactPath && /\.(mp4|mov|webm|gif|png|jpe?g|webp|svg)$/i.test(latestArtifactPath)
    ? (raw.latest_artifact?.download_url ? `${configuredBaseUrl()}${raw.latest_artifact.download_url}` : artifactUrl(latestArtifactPath))
    : undefined;
  const sourceFile = scenes[0]?.file ?? raw.spec.engine?.source ?? raw.files.sources[0] ?? "";
  const accent = typeof (raw.spec as unknown as { theme?: { accent?: string } }).theme?.accent === "string"
    ? (raw.spec as unknown as { theme: { accent: string } }).theme.accent
    : "#159fe8";
  return {
    projectId: raw.project_root,
    projectName: raw.spec.project.title ?? raw.spec.project.name,
    duration,
    fps: raw.spec.render.fps,
    previewUrl: explicitPreview ?? artifactPreview,
    latestArtifactPath,
    captionSourcePath: raw.spec.captions?.source,
    scenes,
    beats,
    assets,
    selection: neutralSelection(sourceFile, scenes[0]?.name ?? "No selection", accent),
    sourceCode: "",
    logs,
    renderQueue: queue,
    exports: artifacts,
    camera: raw.timeline?.camera?.filter((point) => Number.isFinite(point.time) && Number.isFinite(point.value)) ?? [],
    waveform: raw.timeline?.waveform?.filter(Number.isFinite) ?? [],
  };
}

export interface RenderRequest {
  projectId: string;
  sceneId?: string;
  profile: QualityProfile;
  renderer: Renderer;
}

export interface ExportRequest {
  projectId: string;
  format: ExportFormat;
  profile: QualityProfile;
  sourcePath?: string;
}

export type SourceDocument = CompleteSourceDocument;

export class DirectorClient {
  async health(): Promise<boolean> {
    try {
      await requestJson<unknown>("/api/health", {}, 1_800);
      return true;
    } catch {
      return false;
    }
  }

  async getWorkspace(): Promise<WorkspaceState> {
    const [state, logs] = await Promise.all([
      requestJson<ApiState>("/api/state"),
      this.getLogs("", 200).catch(() => []),
    ]);
    return normalizeState(state, logs);
  }

  async sendIntent(_projectId: string, command: string, scope?: string): Promise<RenderJob> {
    const job = await requestJson<ApiJob>("/api/intents", {
      method: "POST",
      body: JSON.stringify({ intent: command, params: scope ? { scope } : {} }),
    }, 30_000);
    return jobToRenderJob(job);
  }

  async render(request: RenderRequest): Promise<RenderJob> {
    const profile = request.profile.toLowerCase().split(" ")[0];
    const job = await requestJson<ApiJob>("/api/renders", {
      method: "POST",
      body: JSON.stringify({ scene: request.sceneId, profile, params: { renderer: request.renderer.toLowerCase() } }),
    }, 12_000);
    return jobToRenderJob(job);
  }

  async cancelRender(renderId: string): Promise<RenderJob | undefined> {
    const value = await requestJson<ApiJob | undefined>(`/api/renders/${encodeURIComponent(renderId)}/cancel`, { method: "POST" });
    return value ? jobToRenderJob(value) : undefined;
  }

  async exportProject(request: ExportRequest): Promise<RenderJob> {
    const leaf = request.projectId.replaceAll("\\", "/").split("/").at(-1) || "manim-project";
    const safeLeaf = leaf.replace(/[^A-Za-z0-9_.-]+/g, "-");
    const extension = request.format === "bundle" || request.format === "captions" ? "zip" : request.format;
    const outputName = request.format === "bundle" ? `${safeLeaf}-source.zip` : request.format === "captions" ? `${safeLeaf}-captions.zip` : `${safeLeaf}.${extension}`;
    const job = await requestJson<ApiJob>("/api/exports", {
      method: "POST",
      body: JSON.stringify({
        format: request.format,
        output: `output/${outputName}`,
        params: {
          profile: request.profile.toLowerCase().split(" ")[0],
          ...(request.sourcePath ? { source: request.sourcePath } : {}),
        },
      }),
    }, 30_000);
    return jobToRenderJob(job);
  }

  async retryJob(job: RenderJob): Promise<RenderJob> {
    const params = { ...(job.request ?? {}) };
    if (job.operation === "render") {
      const { scene, profile, section, ...rest } = params;
      return jobToRenderJob(await requestJson<ApiJob>("/api/renders", { method: "POST", body: JSON.stringify({ scene, profile, section, params: rest }) }, 12_000));
    }
    if (job.operation === "export") {
      const { format, output, ...rest } = params;
      return jobToRenderJob(await requestJson<ApiJob>("/api/exports", { method: "POST", body: JSON.stringify({ format, output, params: rest }) }, 30_000));
    }
    const intent = typeof params.intent === "string" ? params.intent : `Retry ${job.operation ?? "operation"}`;
    delete params.intent;
    return jobToRenderJob(await requestJson<ApiJob>("/api/intents", { method: "POST", body: JSON.stringify({ intent, operation: job.operation, params }) }, 30_000));
  }

  async getLogs(cursor = "", limit = 200): Promise<LogEntry[]> {
    const query = new URLSearchParams({ cursor, limit: String(limit) });
    const page = await requestJson<{ items: ApiLog[] }>(`/api/logs?${query}`);
    return page.items.map(normalizeLog);
  }

  async getSource(path: string): Promise<SourceDocument> {
    return loadCompleteSource(path, async (sourcePath, startLine, endLine) => {
      const query = new URLSearchParams({ path: sourcePath, start_line: String(startLine), end_line: String(endLine) });
      return requestJson<SourcePage>(`/api/state/source?${query}`);
    });
  }

  async saveSource(path: string, content: string, expectedRevision?: string): Promise<{ path: string; revision: string; undo_path: string; affected_scenes: string[] }> {
    if (!path || !expectedRevision) throw new Error("Refusing source save: a complete revision-locked load is required");
    const bytes = sourceByteLength(content);
    if (bytes > MAX_SOURCE_BYTES) throw new Error(`Refusing source save: ${bytes} bytes exceeds the ${MAX_SOURCE_BYTES}-byte source limit`);
    return requestJson("/api/state/source", { method: "PUT", body: JSON.stringify({ path, content, expected_revision: expectedRevision }) }, 15_000);
  }

  subscribe(onEvent: (event: DirectorEvent) => void, onDisconnect: () => void): () => void {
    const stream = new EventSource(`${configuredBaseUrl()}/api/events`);
    const dispatch = (event: MessageEvent) => {
      try {
        onEvent(JSON.parse(event.data) as DirectorEvent);
      } catch {
        // Keepalive events are intentionally not JSON.
      }
    };
    stream.onmessage = dispatch;
    for (const type of ["job_queued", "job_started", "job_progress", "job_finished"] as const) stream.addEventListener(type, dispatch as EventListener);
    stream.onerror = () => { if (stream.readyState === EventSource.CLOSED) onDisconnect(); };
    return () => stream.close();
  }
}

export const directorClient = new DirectorClient();
