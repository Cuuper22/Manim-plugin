import { useCallback, useEffect, useRef, useState } from "react";
import { artifactUrl, directorClient, jobToRenderJob, type ApiJob } from "./client";
import { freshDemoWorkspace } from "../data/demo";
import type {
  ConnectionMode,
  DirectorEvent,
  ExportFormat,
  ExportArtifact,
  QualityProfile,
  RenderJob,
  Renderer,
  SelectedObject,
  WorkspaceState,
} from "../types";

function emptyWorkspace(): WorkspaceState {
  return {
    projectId: "",
    projectName: "Connecting…",
    duration: 0,
    fps: 30,
    scenes: [],
    beats: [],
    assets: [],
    selection: {
      id: "no-selection", name: "No selection", type: "—", visible: true, locked: false,
      position: [0, 0, 0], scale: [1, 1, 1], rotation: 0, anchor: "CENTER",
      color: "#159fe8", fillOpacity: 1, stroke: "#159fe8", strokeWidth: 1,
      appear: 0, start: 0, end: 0, fadeIn: 0, fadeOut: 0, easing: "smooth",
      source: { file: "", line: 1, object: "" },
    },
    sourceCode: "",
    logs: [],
    renderQueue: [],
    exports: [],
    camera: [],
    waveform: [],
  };
}

function nowTime(): string {
  return new Intl.DateTimeFormat("en-US", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date());
}

function mergeEvent(state: WorkspaceState, event: DirectorEvent): WorkspaceState {
  if (event.type === "job_progress" && event.job_id) {
    const data = event.data && typeof event.data === "object" ? event.data as Record<string, unknown> : {};
    return {
      ...state,
      renderQueue: state.renderQueue.map((job) => {
        if (job.id !== event.job_id) return job;
        const frames = Number(data.frame ?? data.frames ?? job.frames);
        const totalFrames = Number(data.total_frames ?? job.totalFrames);
        const progress = Number(data.progress ?? (totalFrames ? (frames / totalFrames) * 100 : job.progress));
        return { ...job, status: "rendering", frames, totalFrames, progress: Math.max(0, Math.min(100, Math.round(progress))) };
      }),
      logs: [{ id: crypto.randomUUID(), time: nowTime(), level: "INFO" as const, message: event.event ?? "Render progress", source: "director.engine" }, ...state.logs].slice(0, 250),
    };
  }
  if (event.job && typeof event.job === "object") {
    const normalized = jobToRenderJob({ ...(event.job as ApiJob), result: event.result as Record<string, unknown> | undefined });
    const normalizedOutput = normalized.output?.replaceAll("\\", "/");
    const normalizedRoot = state.projectId.replaceAll("\\", "/").replace(/\/$/, "");
    const incoming = normalizedOutput && normalizedRoot && normalizedOutput.startsWith(`${normalizedRoot}/`)
      ? { ...normalized, output: normalizedOutput.slice(normalizedRoot.length + 1) }
      : normalized;
    const prior = state.renderQueue.find((job) => job.id === incoming.id);
    const job = prior ? {
      ...prior,
      status: incoming.status,
      progress: incoming.status === "complete" ? 100 : prior.progress,
      frames: incoming.status === "complete" ? prior.totalFrames : prior.frames,
      output: incoming.output ?? prior.output,
    } : incoming;
    const queue = state.renderQueue.some((item) => item.id === job.id)
      ? state.renderQueue.map((item) => (item.id === job.id ? job : item))
      : [job, ...state.renderQueue];
    const exports = event.type === "job_finished" && job.status === "complete" && job.output
      ? [{ id: `export-${job.id}`, name: job.output.split(/[\\/]/).at(-1) ?? job.output, format: job.output.split(".").at(-1)?.toUpperCase() ?? "FILE", size: "on disk", createdAt: nowTime(), url: artifactUrl(job.output) }, ...state.exports]
      : state.exports;
    const previewUrl = job.status === "complete" && job.output && /\.(mp4|mov|webm|gif|png|jpe?g|webp|svg)$/i.test(job.output)
      ? artifactUrl(job.output)
      : state.previewUrl;
    return { ...state, renderQueue: queue, exports, previewUrl, latestArtifactPath: job.output ?? state.latestArtifactPath };
  }
  return state;
}

function demoIntentPatch(state: WorkspaceState, command: string): WorkspaceState {
  const normalized = command.toLowerCase();
  const color = command.match(/#[\da-f]{6}/i)?.[0];
  let selection = state.selection;
  let sourceCode = state.sourceCode;
  let detail = "Applied the direction to the current selection.";

  if (color) {
    selection = { ...selection, color, stroke: color };
    detail = `Updated ${selection.name} to ${color.toUpperCase()}.`;
  } else if (/hold|linger|longer|slow/.test(normalized)) {
    const seconds = Number(command.match(/(\d+(?:\.\d+)?)\s*(?:s|sec|second)/i)?.[1] ?? 2);
    selection = { ...selection, end: Math.min(state.duration, selection.end + seconds) };
    detail = `Extended ${selection.name} by ${seconds.toFixed(1)} seconds.`;
  } else if (/hide|remove/.test(normalized)) {
    selection = { ...selection, visible: false };
    detail = `Hidden ${selection.name}.`;
  } else if (/show|reveal/.test(normalized)) {
    selection = { ...selection, visible: true };
    detail = `Made ${selection.name} visible.`;
  } else if (/bigger|increase|scale up/.test(normalized)) {
    selection = { ...selection, scale: selection.scale.map((value) => Number((value * 1.15).toFixed(2))) as [number, number, number] };
    detail = `Scaled ${selection.name} up by 15%.`;
  } else if (/vertical|9:16/.test(normalized)) {
    sourceCode = `${sourceCode.trimEnd()}\n\n# Director layout: vertical-safe composition (9:16)\n`;
    detail = "Enabled the vertical-safe composition pass.";
  }

  return {
    ...state,
    selection,
    sourceCode,
    logs: [{ id: crypto.randomUUID(), time: nowTime(), level: "INFO", message: detail, source: "director.intent" }, ...state.logs],
  };
}

export function useDirectorWorkspace() {
  const [workspace, setWorkspace] = useState<WorkspaceState>(() => emptyWorkspace());
  const [connection, setConnection] = useState<ConnectionMode>("connecting");
  const [message, setMessage] = useState("Connecting to Director engine…");
  const [sourceLoad, setSourceLoad] = useState<{
    path: string;
    revision?: string;
    bytes: number;
    complete: boolean;
  }>({ path: "", bytes: 0, complete: false });
  const timers = useRef(new Map<string, number>());

  useEffect(() => {
    let active = true;
    let unsubscribe: () => void = () => {};
    void (async () => {
      const available = await directorClient.health();
      if (!active) return;
      if (!available) {
        setWorkspace(freshDemoWorkspace());
        setConnection("demo");
        setMessage("Local demo · engine offline");
        return;
      }
      try {
        const next = await directorClient.getWorkspace();
        if (!active) return;
        setWorkspace(next);
        setConnection("online");
        setMessage("Director engine connected");
        unsubscribe = directorClient.subscribe(
          (event) => setWorkspace((current) => mergeEvent(current, event)),
          () => setMessage("Event stream reconnecting…"),
        );
      } catch (error) {
        if (!active) return;
        setWorkspace(freshDemoWorkspace());
        setConnection("demo");
        setMessage(`Local demo · ${error instanceof Error ? error.message : "engine unavailable"}`);
      }
    })();
    return () => {
      active = false;
      unsubscribe();
      for (const timer of timers.current.values()) window.clearInterval(timer);
      timers.current.clear();
    };
  }, []);

  useEffect(() => {
    const path = workspace.selection.source.file;
    if (connection !== "online") {
      setSourceLoad({ path, bytes: 0, complete: false });
      return;
    }

    let active = true;
    setSourceLoad({ path, bytes: 0, complete: false });
    setWorkspace((state) => ({ ...state, sourceCode: "" }));
    if (!path) return () => { active = false; };

    setMessage(`Loading complete source · ${path}`);
    void directorClient.getSource(path).then((document) => {
      if (!active) return;
      setWorkspace((state) => ({ ...state, sourceCode: document.content }));
      setSourceLoad({
        path: document.path,
        revision: document.revision,
        bytes: document.bytes,
        complete: document.complete,
      });
      setMessage(`Source loaded · ${document.total_lines.toLocaleString()} lines`);
    }).catch((error: unknown) => {
      if (!active) return;
      setSourceLoad({ path, bytes: 0, complete: false });
      setMessage(error instanceof Error ? error.message : "Source could not be loaded completely");
    });
    return () => { active = false; };
  }, [connection, workspace.selection.source.file]);

  const sourceReady = connection === "demo" || (
    sourceLoad.complete
    && sourceLoad.path === workspace.selection.source.file
    && Boolean(sourceLoad.revision)
  );

  const runIntent = useCallback(async (command: string, scope?: string) => {
    if (!command.trim()) return;
    setMessage("Director is routing your instruction…");
    if (connection === "demo") {
      setWorkspace((state) => demoIntentPatch(state, command.trim()));
      setMessage("Direction applied locally");
      return;
    }
    try {
      const job = await directorClient.sendIntent(workspace.projectId, command.trim(), scope);
      setWorkspace((state) => ({
        ...state,
        renderQueue: [job, ...state.renderQueue.filter((item) => item.id !== job.id)],
        logs: [{ id: crypto.randomUUID(), time: nowTime(), level: "INFO", message: `Director queued: ${command.trim()}`, source: "director.intent" }, ...state.logs],
      }));
      setMessage("Direction routed to the engine");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Direction failed");
    }
  }, [connection, workspace.projectId]);

  const startRender = useCallback(async (profile: QualityProfile, renderer: Renderer, sceneId?: string) => {
    setMessage("Starting render…");
    if (connection === "online") {
      try {
        const selectedScene = workspace.scenes.find((item) => item.id === sceneId);
        const job = await directorClient.render({ projectId: workspace.projectId, sceneId: selectedScene?.name ?? sceneId, profile, renderer });
        setWorkspace((state) => ({ ...state, renderQueue: [job, ...state.renderQueue.filter((item) => item.id !== job.id)] }));
        setMessage(`Rendering ${job.scene}`);
        return;
      } catch (error) {
        setMessage(error instanceof Error ? error.message : "Render failed to start");
        return;
      }
    }

    const scene = workspace.scenes.find((item) => item.id === sceneId) ?? workspace.scenes[0];
    const id = `local-${Date.now()}`;
    const totalFrames = Math.max(1, Math.round((scene.end - scene.start) * workspace.fps));
    const job: RenderJob = { id, scene: scene.name, profile, renderer, status: "rendering", progress: 0, frames: 0, totalFrames };
    setWorkspace((state) => ({
      ...state,
      renderQueue: [job, ...state.renderQueue],
      logs: [{ id: crypto.randomUUID(), time: nowTime(), level: "INFO", message: `Render started: ${scene.name} (${profile}, ${renderer})`, source: "director.local" }, ...state.logs],
    }));
    const timer = window.setInterval(() => {
      setWorkspace((state) => {
        const current = state.renderQueue.find((item) => item.id === id);
        if (!current || current.status !== "rendering") return state;
        const frames = Math.min(totalFrames, current.frames + Math.max(2, Math.round(totalFrames / 22)));
        const complete = frames >= totalFrames;
        if (complete) {
          window.clearInterval(timers.current.get(id));
          timers.current.delete(id);
          setMessage("Preview render complete");
        }
        return {
          ...state,
          renderQueue: state.renderQueue.map((item) => item.id === id ? {
            ...item,
            frames,
            progress: Math.round((frames / totalFrames) * 100),
            status: complete ? "complete" : "rendering",
            output: complete ? `media/videos/${scene.id}/${scene.name.replaceAll(" ", "")}.mp4` : undefined,
          } : item),
          logs: complete ? [{ id: crypto.randomUUID(), time: nowTime(), level: "INFO", message: `Render completed: ${scene.name} (${totalFrames} frames)`, source: "director.local" }, ...state.logs] : state.logs,
        };
      });
    }, 180);
    timers.current.set(id, timer);
  }, [connection, workspace.fps, workspace.projectId, workspace.scenes]);

  const cancelRender = useCallback(async (id: string) => {
    if (connection === "online") {
      try {
        const updated = await directorClient.cancelRender(id);
        if (updated) setWorkspace((state) => ({ ...state, renderQueue: state.renderQueue.map((job) => job.id === id ? updated : job) }));
        setMessage("Render cancelled");
      } catch (error) {
        setMessage(error instanceof Error ? error.message : "Could not cancel render");
      }
      return;
    }
    window.clearInterval(timers.current.get(id));
    timers.current.delete(id);
    setWorkspace((state) => ({ ...state, renderQueue: state.renderQueue.map((job) => job.id === id ? { ...job, status: "cancelled" } : job) }));
    setMessage("Render cancelled");
  }, [connection]);

  const retryRender = useCallback(async (id: string) => {
    const original = workspace.renderQueue.find((job) => job.id === id);
    if (!original) return;
    setMessage(`Retrying ${original.scene}…`);
    if (connection === "online") {
      try {
        const retried = await directorClient.retryJob(original);
        setWorkspace((state) => ({ ...state, renderQueue: [retried, ...state.renderQueue] }));
        setMessage(`${original.scene} queued again`);
      } catch (error) {
        setMessage(error instanceof Error ? error.message : "Retry failed");
      }
      return;
    }
    const profile = (["Draft 480p", "Preview 720p", "Production 1080p", "Cinema 4K"].includes(original.profile) ? original.profile : "Preview 720p") as QualityProfile;
    const renderer = original.renderer.toLowerCase() === "opengl" ? "OpenGL" : "Cairo";
    const scene = workspace.scenes.find((item) => item.name === original.scene);
    await startRender(profile, renderer, scene?.id);
  }, [connection, startRender, workspace.renderQueue, workspace.scenes]);

  const exportProject = useCallback(async (format: ExportFormat) => {
    setMessage(`Preparing ${format.toUpperCase()} export…`);
    if (connection === "online") {
      try {
        const mediaFormat = format === "mp4" || format === "webm" || format === "gif";
        const captionSource = workspace.captionSourcePath ?? workspace.assets.find((asset) => /\.(vtt|srt)$/i.test(asset.path))?.path;
        const queuedVideo = workspace.renderQueue.find((job) => job.status === "complete" && job.output && /\.(mp4|mov|webm|gif|mkv|avi|m4v)$/i.test(job.output))?.output;
        const sourcePath = mediaFormat
          ? (/\.(mp4|mov|webm|gif|mkv|avi|m4v)$/i.test(workspace.latestArtifactPath ?? "") ? workspace.latestArtifactPath : queuedVideo)
          : format === "captions" ? captionSource : undefined;
        if (mediaFormat && (!sourcePath || !/\.(mp4|mov|webm|gif|mkv|avi|m4v)$/i.test(sourcePath))) {
          setMessage("Render a video before exporting media formats");
          return;
        }
        if (format === "captions" && !sourcePath) {
          setMessage("Add a VTT or SRT caption source before exporting captions");
          return;
        }
        const job = await directorClient.exportProject({ projectId: workspace.projectId, format, profile: "Production 1080p", sourcePath });
        setWorkspace((state) => ({ ...state, renderQueue: [job, ...state.renderQueue.filter((item) => item.id !== job.id)] }));
        setMessage(`${format.toUpperCase()} export queued`);
      } catch (error) {
        setMessage(error instanceof Error ? error.message : "Export failed");
      }
      return;
    }
    const name = format === "bundle" ? `${workspace.projectId}.director.json` : `${workspace.projectId}-${format}-manifest.json`;
    const blob = new Blob([JSON.stringify({ format, exportedAt: new Date().toISOString(), workspace }, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = name;
    anchor.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
    const artifact: ExportArtifact = { id: crypto.randomUUID(), name, format: format.toUpperCase(), size: `${Math.ceil(blob.size / 1024)} KB`, createdAt: nowTime(), url };
    setWorkspace((state) => ({ ...state, exports: [artifact, ...state.exports] }));
    setMessage(`${name} exported`);
  }, [connection, workspace]);

  const updateSelection = useCallback((selection: SelectedObject, persist = false) => {
    setWorkspace((state) => ({ ...state, selection }));
    if (persist && connection === "demo") setMessage("Object edit applied locally");
  }, [connection]);

  const saveSource = useCallback(async (source: string): Promise<boolean> => {
    const path = workspace.selection.source.file;
    if (connection === "demo") {
      setWorkspace((state) => ({ ...state, sourceCode: source }));
      setMessage("Source saved in this session");
      return true;
    }
    if (connection !== "online" || !path || !sourceLoad.complete || sourceLoad.path !== path || !sourceLoad.revision) {
      setMessage("Refusing source save · complete revision-locked source is not loaded");
      return false;
    }

    try {
      const result = await directorClient.saveSource(path, source, sourceLoad.revision);
      setWorkspace((state) => ({ ...state, sourceCode: source }));
      setSourceLoad({ path: result.path, revision: result.revision, bytes: new TextEncoder().encode(source).byteLength, complete: true });
      setMessage(`Source saved · undo snapshot ${result.undo_path}`);
      return true;
    } catch (error) {
      const detail = error instanceof Error ? error.message : "Source save failed";
      if (!(error instanceof Error) || /revision conflict|source changed|abort|network|failed to fetch/i.test(detail)) {
        setSourceLoad({ path, bytes: 0, complete: false });
      }
      setMessage(detail);
      return false;
    }
  }, [connection, sourceLoad, workspace.selection.source.file]);

  return {
    workspace,
    setWorkspace,
    connection,
    message,
    sourceReady,
    setMessage,
    runIntent,
    startRender,
    cancelRender,
    retryRender,
    exportProject,
    updateSelection,
    saveSource,
  };
}
