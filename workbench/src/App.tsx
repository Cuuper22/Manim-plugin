import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useDirectorWorkspace } from "./api/useDirectorWorkspace";
import { BottomPanel, StatusBar } from "./components/BottomPanel";
import { ActivityRail, ProjectExplorer, TopBar } from "./components/Chrome";
import { Inspector } from "./components/Inspector";
import { Stage } from "./components/Stage";
import { Timeline } from "./components/Timeline";
import type { BeatItem, BottomTab, InspectorTab, LeftTab, QualityProfile, Renderer, SceneItem } from "./types";

export function App() {
  const {
    workspace,
    setWorkspace,
    connection,
    message,
    sourceReady,
    runIntent,
    startRender,
    cancelRender,
    retryRender,
    exportProject,
    updateSelection,
    saveSource,
  } = useDirectorWorkspace();
  const [currentTime, setCurrentTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [timelineZoom, setTimelineZoom] = useState(1);
  const [leftTab, setLeftTab] = useState<LeftTab>("project");
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("inspector");
  const [bottomTab, setBottomTab] = useState<BottomTab>("console");
  const [selectedSceneId, setSelectedSceneId] = useState(workspace.scenes[0]?.id ?? "");
  const [selectedBeatId, setSelectedBeatId] = useState<string>();
  const [activeTool, setActiveTool] = useState("project");
  const [profile, setProfile] = useState<QualityProfile>("Preview 720p");
  const [renderer, setRenderer] = useState<Renderer>("Cairo");
  const [projectOpen, setProjectOpen] = useState(true);
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const animationFrame = useRef<number | undefined>(undefined);
  const previousTimestamp = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (workspace.scenes.length && !workspace.scenes.some((scene) => scene.id === selectedSceneId)) {
      setSelectedSceneId(workspace.scenes[0].id);
    } else if (!workspace.scenes.length && selectedSceneId) {
      setSelectedSceneId("");
    }
  }, [selectedSceneId, workspace.scenes]);

  useEffect(() => {
    setCurrentTime(connection === "demo" ? Math.min(3.42, workspace.duration) : 0);
    setPlaying(false);
  }, [connection, workspace.projectId, workspace.duration]);

  useEffect(() => {
    if (!playing) {
      previousTimestamp.current = undefined;
      if (animationFrame.current) cancelAnimationFrame(animationFrame.current);
      return;
    }
    const advance = (timestamp: number) => {
      const previous = previousTimestamp.current ?? timestamp;
      previousTimestamp.current = timestamp;
      setCurrentTime((time) => {
        const next = time + (timestamp - previous) / 1000;
        if (next >= workspace.duration) {
          setPlaying(false);
          return workspace.duration;
        }
        return next;
      });
      animationFrame.current = requestAnimationFrame(advance);
    };
    animationFrame.current = requestAnimationFrame(advance);
    return () => {
      if (animationFrame.current) cancelAnimationFrame(animationFrame.current);
    };
  }, [playing, workspace.duration]);

  const renderSelected = useCallback(() => {
    if (!selectedSceneId) return;
    void startRender(profile, renderer, selectedSceneId);
    setBottomTab("queue");
  }, [profile, renderer, selectedSceneId, startRender]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement;
      const isTyping = target.matches("input, textarea, select, [contenteditable='true']");
      if (event.code === "Space" && !isTyping && workspace.duration > 0) {
        event.preventDefault();
        setPlaying((value) => !value);
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        document.querySelector<HTMLInputElement>("[data-command-input]")?.focus();
      }
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
        event.preventDefault();
        renderSelected();
      }
      if (event.key === "Escape") {
        setProjectOpen(false);
        setInspectorOpen(false);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [renderSelected, workspace.duration]);

  const selectScene = (scene: SceneItem) => {
    setSelectedSceneId(scene.id);
    setCurrentTime(scene.start);
    setPlaying(false);
    updateSelection({
      ...workspace.selection,
      name: scene.name,
      type: "Scene",
      start: scene.start,
      end: scene.end,
      source: { file: scene.file, line: 1, object: scene.name.replaceAll(" ", "") },
    });
  };

  const selectBeat = (beat: BeatItem) => {
    setSelectedBeatId(beat.id);
    setCurrentTime(beat.start);
    setPlaying(false);
    if (beat.source) {
      updateSelection({
        ...workspace.selection,
        name: beat.source.object,
        type: beat.kind === "caption" ? "Caption" : "AnimationGroup",
        start: beat.start,
        end: beat.end,
        appear: beat.start,
        source: beat.source,
      });
    }
  };

  const handleActivity = (action: string) => {
    setActiveTool(action);
    if (action === "project" || action === "outline") {
      setProjectOpen(true);
      setLeftTab("project");
    } else if (action === "audio") {
      setProjectOpen(true);
      setLeftTab("assets");
    } else if (action === "code") {
      setInspectorOpen(true);
      setInspectorTab("code");
    } else {
      setInspectorOpen(true);
      setInspectorTab("inspector");
    }
  };

  const activeJob = useMemo(() => workspace.renderQueue.find((job) => job.status === "rendering" || job.status === "queued"), [workspace.renderQueue]);
  const captions = useMemo(() => workspace.beats.filter((beat) => beat.kind === "caption"), [workspace.beats]);
  const mediaExportAvailable = useMemo(() => {
    if (/\.(mp4|mov|webm|gif|mkv|avi|m4v)$/i.test(workspace.latestArtifactPath ?? "")) return true;
    return workspace.renderQueue.some((job) => job.status === "complete" && /\.(mp4|mov|webm|gif|mkv|avi|m4v)$/i.test(job.output ?? ""));
  }, [workspace.latestArtifactPath, workspace.renderQueue]);
  const captionExportAvailable = useMemo(() => Boolean(workspace.captionSourcePath) || workspace.assets.some((asset) => /\.(vtt|srt)$/i.test(asset.path)), [workspace.assets, workspace.captionSourcePath]);

  return (
    <main className="director-app">
      <TopBar
        projectName={workspace.projectName}
        profile={profile}
        renderer={renderer}
        connection={connection}
        rendering={Boolean(activeJob?.status === "rendering")}
        canRender={Boolean(selectedSceneId)}
        canSave={Boolean(workspace.selection.source.file) && sourceReady}
        onProfileChange={setProfile}
        onRendererChange={setRenderer}
        onRender={renderSelected}
        onExport={() => { void exportProject("bundle"); setBottomTab("exports"); }}
        onToggleProject={() => setProjectOpen((value) => !value)}
        onToggleInspector={() => setInspectorOpen((value) => !value)}
        onSave={() => void saveSource(workspace.sourceCode)}
        onSnapshot={() => { void exportProject("bundle"); setBottomTab("exports"); }}
      />
      <div className="workbench-body">
        <ActivityRail active={activeTool} onAction={handleActivity} />
        <ProjectExplorer
          open={projectOpen}
          projectName={workspace.projectName}
          tab={leftTab}
          scenes={workspace.scenes}
          assets={workspace.assets}
          selectedSceneId={selectedSceneId}
          onTabChange={setLeftTab}
          onSceneSelect={selectScene}
          onClose={() => setProjectOpen(false)}
        />

        <div className="editor-column">
          <Stage
            currentTime={currentTime}
            duration={workspace.duration}
            playing={playing}
            fps={workspace.fps}
            previewUrl={workspace.previewUrl}
            demoMode={connection === "demo"}
            captions={captions}
            onPlayingChange={setPlaying}
            onTimeChange={setCurrentTime}
          />
          <Timeline
            currentTime={currentTime}
            duration={workspace.duration}
            scenes={workspace.scenes}
            beats={workspace.beats}
            waveform={workspace.waveform}
            camera={workspace.camera}
            zoom={timelineZoom}
            selectedBeatId={selectedBeatId}
            onZoomChange={setTimelineZoom}
            onSeek={setCurrentTime}
            onBeatSelect={selectBeat}
            onRunIntent={(command) => runIntent(command, selectedBeatId ?? selectedSceneId)}
          />
          <BottomPanel
            tab={bottomTab}
            logs={workspace.logs}
            jobs={workspace.renderQueue}
            exports={workspace.exports}
            onTabChange={setBottomTab}
            onClearLogs={() => setWorkspace((state) => ({ ...state, logs: [] }))}
            onCancelRender={(id) => void cancelRender(id)}
            onRetryRender={(id) => void retryRender(id)}
            onExport={(format) => void exportProject(format)}
            mediaExportAvailable={mediaExportAvailable}
            captionExportAvailable={captionExportAvailable}
          />
        </div>

        <Inspector
          open={inspectorOpen}
          tab={inspectorTab}
          selection={workspace.selection}
          sourceCode={workspace.sourceCode}
          editableVisuals={connection === "demo"}
          sourceReady={sourceReady}
          onTabChange={setInspectorTab}
          onSelectionChange={(selection, persist) => {
            if (connection === "demo") updateSelection(selection, persist);
          }}
          onSourceSave={saveSource}
          onClose={() => setInspectorOpen(false)}
        />
        {projectOpen || inspectorOpen ? <button type="button" className="drawer-scrim" aria-label="Close panels" onClick={() => { setProjectOpen(false); setInspectorOpen(false); }} /> : null}
      </div>
      <StatusBar
        projectName={workspace.projectName}
        sceneCount={workspace.scenes.length}
        duration={workspace.duration}
        fps={workspace.fps}
        message={message}
        activeJob={activeJob}
        onCancel={(id) => void cancelRender(id)}
      />
    </main>
  );
}
