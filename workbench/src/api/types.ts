// Wire types of the engine's HTTP API v2 (CONTRACT-http §5), checked against
// the engine's serializers. Fields typed `T | null` are always present.

export type Iso8601 = string;
export type JobId = string;
/** `"<file>#<Class>"`. */
export type SceneId = string;
/** Project-relative POSIX path; `"."` only for a directory equal to the root. */
export type ProjectPath = string;
/** Absolute host path, for display only. */
export type HostPath = string;

export interface LineSpan {
  start: number;
  end: number;
}

// ── operations ──────────────────────────────────────────────────────────────

export type OperationName =
  | "init" | "discover" | "doctor" | "render" | "still" | "frame" | "contact_sheet"
  | "qa" | "diagnose" | "validate_math" | "captions" | "ingest" | "export";
export type JobOperation = Exclude<OperationName, "init" | "discover">;
export type JobStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export type JobOrigin = "http" | "mcp" | "cli" | "engine";
export type Severity = "error" | "warning" | "info";
export type ArtifactKind =
  | "video" | "section" | "image" | "contact_sheet" | "captions" | "timeline" | "archive" | "file";
export type Renderer = "cairo" | "opengl";
export type ExportFormat = "zip" | "mp4" | "webm" | "gif";

export type SourceRef = { job_id: JobId } | { path: ProjectPath };

/** The body of `POST /api/jobs`: one operation the engine accepts over HTTP. */
export type OperationRequest =
  | { operation: "doctor" }
  | { operation: "render"; scene?: string; file?: ProjectPath; profile?: string; sections?: boolean; fresh?: boolean }
  | { operation: "still"; scene?: string; file?: ProjectPath; profile?: string; fresh?: boolean }
  | { operation: "frame"; at_seconds: number; source?: SourceRef; scene?: string; profile?: string }
  | { operation: "contact_sheet"; source?: SourceRef; scene?: string; profile?: string; count?: number; columns?: number }
  | { operation: "qa"; source?: SourceRef; scene?: string; profile?: string; frames?: number }
  | { operation: "diagnose"; job_id: JobId }
  | { operation: "diagnose"; text: string }
  | {
      operation: "validate_math";
      steps: string[];
      ranges?: Record<string, [number, number]>;
      samples?: number;
      tolerance?: number;
      seed?: number;
    }
  | { operation: "captions"; path: ProjectPath; shift_seconds?: number; scale?: number; output?: ProjectPath }
  | {
      operation: "export";
      format?: ExportFormat;
      source?: SourceRef;
      scene?: string;
      profile?: string;
      output?: ProjectPath;
      gif_fps?: number;
      gif_width?: number;
    };

/** Submitted through the CLI or MCP only; it reads host paths. */
export interface IngestRequest {
  operation: "ingest";
  sources: { path: HostPath; id?: string; license?: string; attribution?: string }[];
  normalize?: boolean;
  force?: boolean;
}

export type JobRequest = OperationRequest | IngestRequest;

// ── shared result parts ─────────────────────────────────────────────────────

export interface MediaInfo {
  /** `mp4`, `mov`, `webm`, `gif` or `png` for renders; qa sources may also be `jpeg` or `webp`. */
  container: string;
  /** ffprobe `codec_name`, e.g. `h264`, `vp9`, `av1`, `prores`, `qtrle`. */
  codec: string | null;
  width: number;
  height: number;
  fps: number | null;
  duration_seconds: number | null;
  has_alpha: boolean;
}

/** An artifact as a result reports it. */
export interface OpsArtifact {
  kind: ArtifactKind;
  path: ProjectPath;
  label: string | null;
  bytes: number;
  media: MediaInfo | null;
}

/** An artifact the workbench can fetch: `url` is versioned and Range-capable. */
export interface Artifact extends OpsArtifact {
  /** `/api/files/<encoded path>?v=<version>`; answers `410 artifact_changed` once the file changes. */
  url: string;
  content_type: string;
  version: string;
  scene_id: SceneId | null;
}

/** Jump to it only when `file` is a `ProjectPath` (relative). */
export interface SourceLocation {
  file: ProjectPath | HostPath;
  line: number;
  column: number | null;
}

export interface OpsFinding {
  code: string;
  severity: Severity;
  message: string;
  hint: string | null;
  location: SourceLocation | null;
  at_seconds: number | null;
  beat: string | null;
  frame: ProjectPath | null;
}

export type ProgressPhase =
  | "starting" | "import" | "animate" | "encode" | "extract" | "analyze"
  | "package" | "transcode" | "ingest" | "validate";

export interface Progress {
  phase: ProgressPhase;
  current: number;
  /** `null` while indeterminate. */
  total: number | null;
  scene_seconds: number | null;
  message: string | null;
  updated_at: Iso8601;
}

export interface LogEntry {
  cursor: string;
  timestamp: Iso8601;
  stream: "runtime" | "stderr" | "engine";
  level: "info" | "warning" | "error";
  message: string;
  data: Record<string, unknown> | null;
}

/** The error envelope payload, also used for failed and cancelled jobs. Branch on `code` only. */
export interface ErrorBody {
  code: string;
  message: string;
  data: Record<string, unknown> | null;
}

export interface SheetFrame {
  at_seconds: number;
  beat: string | null;
}

// ── operation results (only those read here) ────────────────────────────────

export interface DoctorResult {
  ok: boolean;
  runtime: { version: string; protocol: number; python: string; executable: HostPath; platform: string };
  checks: {
    name: string;
    kind: "package" | "executable";
    available: boolean;
    version: string | null;
    path: HostPath | null;
  }[];
  capabilities: {
    render: boolean;
    renderers: Renderer[];
    latex: boolean;
    video_tools: boolean;
    visual_qa: boolean;
    symbolic_math: boolean;
    pdf_ingest: boolean;
  };
  disk: { free_bytes: number; total_bytes: number };
  findings: OpsFinding[];
  artifacts: OpsArtifact[];
}

export interface DiagnoseResult {
  recognized: boolean;
  findings: OpsFinding[];
  artifacts: OpsArtifact[];
}

// ── jobs ────────────────────────────────────────────────────────────────────

export interface JobSummary {
  id: JobId;
  sequence: number;
  operation: JobOperation;
  status: JobStatus;
  origin: JobOrigin;
  cached: boolean;
  cached_from: JobId | null;
  source_job_id: JobId | null;
  cancel_requested: boolean;
  /** As stored; re-POST it to retry. */
  request: JobRequest;
  scene_id: SceneId | null;
  profile: string | null;
  created_at: Iso8601;
  started_at: Iso8601 | null;
  finished_at: Iso8601 | null;
  /** Non-null only while running. */
  progress: Progress | null;
  /** Non-null iff failed or cancelled. */
  error: ErrorBody | null;
  /** At most 20, by kind priority; empty unless succeeded. */
  artifacts: Artifact[];
  artifacts_total: number;
}

/** One job with every artifact still on disk; `result` is non-null iff succeeded. Only a diagnosis's is read. */
export type Job =
  | (JobSummary & { operation: "diagnose"; result: DiagnoseResult | null })
  | (JobSummary & { operation: Exclude<JobOperation, "diagnose">; result: unknown });

export interface JobPage {
  items: JobSummary[];
  next_before: string | null;
}

export interface LogPage {
  items: LogEntry[];
  next_after: string | null;
}

// ── workspace ───────────────────────────────────────────────────────────────

export interface Health {
  ok: true;
  version: string;
  api_version: number;
  instance_id: string;
}

export interface EngineInfo {
  version: string;
  api_version: number;
  /** Never contains `.`; the prefix of every event id. */
  instance_id: string;
  started_at: Iso8601;
  limits: {
    request_body_bytes: number;
    source_body_bytes: number;
    source_file_bytes: number;
    source_page_lines: number;
  };
}

export interface ProjectSummary {
  root: HostPath;
  name: string;
  description: string | null;
  spec_file: "director.yaml";
  source_dir: ProjectPath;
  asset_dir: ProjectPath;
  output_dir: ProjectPath;
  media_dir: ProjectPath;
  theme: string | null;
  default_profile: string;
  duration_seconds: number | null;
  counts: { sources: number; assets: number; outputs: number };
}

export interface SpecStatus {
  path: "director.yaml";
  valid: boolean;
  revision: string | null;
  /** Non-null iff `valid` is false. */
  error: { message: string; line: number | null; column: number | null } | null;
}

export interface Profile {
  name: string;
  origin: "builtin" | "project";
  width: number;
  height: number;
  fps: number;
  renderer: Renderer;
  format: "mp4" | "mov" | "webm" | "gif" | "png";
  transparent: boolean;
  is_default: boolean;
}

export interface Theme {
  name: string;
  /** Catalog order; colors are `#RRGGBB`. */
  tokens: { token: string; color: string }[];
  is_default: boolean;
}

export interface SceneIndexStatus {
  state: "indexing" | "ready" | "failed";
  indexed_at: Iso8601 | null;
  files: number;
  truncated: boolean;
  /** Non-null iff `state` is `failed`. */
  error: ErrorBody | null;
}

export interface Scene {
  id: SceneId;
  class_name: string;
  file: ProjectPath;
  span: LineSpan;
  construct_line: number | null;
  bases: string[];
  theme: string | null;
  summary: string | null;
  sections: { name: string | null; line: number }[];
  beats: { name: string | null; span: LineSpan }[];
  declared: { id: string; purpose: string | null; duration_seconds: number | null } | null;
  /** The file no longer parses; this entry is its last good parse. */
  parse_failed: boolean;
}

export interface StoryboardBeat {
  id: string;
  intent: "introduce" | "explain" | "compare" | "reveal" | "prove" | "recap" | null;
  transition: "continue" | "contrast" | "reveal" | "chapter" | null;
  audience_question: string | null;
  takeaway: string | null;
  focus: string | null;
  visual_metaphor: string | null;
  duration_seconds: number | null;
  /** `null` when an earlier beat has no duration. */
  start_seconds: number | null;
  code: { scene_id: SceneId; line: number } | null;
}

export interface TimelineMark {
  kind: "beat" | "section";
  name: string;
  start_seconds: number;
  end_seconds: number;
  file: ProjectPath | null;
  line: number | null;
}

export interface LatestBase {
  job_id: JobId;
  finished_at: Iso8601 | null;
  artifact: Artifact;
  /** The scene file changed since the job. */
  outdated: boolean;
}

export interface SceneLatest {
  video: (LatestBase & { profile: string | null; timeline: TimelineMark[] }) | null;
  /** `at_seconds` is `null` for a `still` (the last frame). */
  still: (LatestBase & { operation: "still" | "frame"; at_seconds: number | null }) | null;
  contact_sheet: (LatestBase & { frames: SheetFrame[]; source_job_id: JobId | null }) | null;
}

export interface Finding extends OpsFinding {
  /** Stable within a snapshot. */
  id: string;
  source: "spec" | "index" | "render" | "qa" | "doctor";
  scene_id: SceneId | null;
  job_id: JobId | null;
  outdated: boolean;
  frame_url: string | null;
}

export interface DoctorSnapshot {
  job_id: JobId;
  finished_at: Iso8601 | null;
  report: DoctorResult;
}

/** The sections a `workspace` event replaces wholesale. */
export interface WorkspaceSections {
  project: ProjectSummary;
  spec: SpecStatus;
  profiles: Profile[];
  themes: Theme[];
  scene_index: SceneIndexStatus;
  scenes: Scene[];
  storyboard: StoryboardBeat[];
  /** Exactly the ids in `scenes`. */
  latest: Record<SceneId, SceneLatest>;
  findings: Finding[];
  doctor: DoctorSnapshot | null;
}

export interface WorkspaceState extends WorkspaceSections {
  engine: EngineInfo;
  /** The id of the newest event this snapshot includes. */
  event_cursor: string;
  /** Newest first, at most 50. */
  jobs: JobSummary[];
  jobs_next_before: string | null;
}

// ── source ──────────────────────────────────────────────────────────────────

export type SourceLanguage =
  | "python" | "json" | "yaml" | "toml" | "markdown" | "latex" | "typst" | "captions" | "text";

export interface SourcePage {
  path: ProjectPath;
  /** blake3 of the whole file. */
  revision: string;
  language: SourceLanguage;
  eol: "lf" | "crlf";
  final_newline: boolean;
  /** Size of the whole file. */
  bytes: number;
  total_lines: number;
  start_line: number;
  /** Inclusive; `start_line - 1` for an empty page. */
  end_line: number;
  /** Lines `start_line..end_line` joined by `"\n"`, without a trailing terminator. */
  content: string;
}

export type SourceEdit =
  | { kind: "replace_all"; content: string }
  | { kind: "replace_lines"; start_line: number; end_line: number; replacement: string }
  | { kind: "merge_patch"; patch: Record<string, unknown> };

export interface SourceWrite {
  path: ProjectPath;
  /** `null` creates the file, which must not exist yet. */
  expected_revision: string | null;
  edit: SourceEdit;
}

export interface SourceWriteResult {
  path: ProjectPath;
  previous_revision: string | null;
  revision: string;
  bytes: number;
  total_lines: number;
  affected_scenes: SceneId[];
}

// ── events ──────────────────────────────────────────────────────────────────

export type ResyncReason = "unknown_cursor" | "expired" | "lagged";

export type ServerEvent =
  | { type: "job"; job: JobSummary }
  | { type: "progress"; job_id: JobId; progress: Progress }
  | { type: "workspace"; sections: Partial<WorkspaceSections> }
  /** `revision` is `null` once the file is deleted. */
  | { type: "file"; path: ProjectPath; revision: string | null }
  | { type: "resync"; reason: ResyncReason };

export type ServerEventType = ServerEvent["type"];
