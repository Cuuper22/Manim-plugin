import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ExportFormat, JobId, JobSummary, Scene, SceneId, SceneLatest } from "../api/types.ts";
import type { EditorApi, OpenRequest } from "../editor/CodeEditor.tsx";
import { useNewestDiagnosis } from "../hooks/useDiagnosis.ts";
import { useLaunches, type Launch } from "../hooks/useLaunches.ts";
import { useSelection } from "../hooks/useSelection.ts";
import { useShortcuts } from "../hooks/useShortcuts.ts";
import { useTheme } from "../hooks/useTheme.ts";
import { planAction, planExport, type StageAction } from "../model/actions.ts";
import { bySeverity, fromWorkspace, type CodeTarget } from "../model/findings.ts";
import { isActive, progressFraction, stageActivity } from "../model/jobs.ts";
import { PlaybackController } from "../stage/playback.ts";
import type { Workspace } from "../store/reducer.ts";
import { useWorkbench } from "../store/useWorkbench.ts";
import { ActionBar, type ActionState } from "./ActionBar.tsx";
import { FindingsPane } from "./FindingsPane.tsx";
import { Inspector, type InspectorTab } from "./Inspector.tsx";
import { JobTray } from "./JobTray.tsx";
import { LogsDialog } from "./LogsDialog.tsx";
import { ScenePanel } from "./ScenePanel.tsx";
import { ShortcutsDialog } from "./ShortcutsDialog.tsx";
import { isPlayable, Stage, type StageView } from "./Stage.tsx";
import { TopBar } from "./TopBar.tsx";

/** On narrow screens one region shows at a time; wide screens show all three. */
type Region = "scenes" | "stage" | "inspector";

/** The view a successful job from these buttons brings up. */
const VIEW_AFTER: Partial<Record<string, StageView>> = {
  preview: "video",
  render: "video",
  still: "still",
  frame: "still",
  contact_sheet: "sheet",
};

/** The render when there is one, else whatever exists. */
function firstView(latest: SceneLatest | null): StageView {
  if (!latest || latest.video) return "video";
  if (latest.still) return "still";
  return latest.contact_sheet ? "sheet" : "video";
}

interface WorkbenchProps {
  workspace: Workspace;
  /** The event stream is reconnecting; what shows may be out of date. */
  reconnecting: boolean;
  /** The engine cannot be used: hidden and inert, but kept so nothing typed is lost. */
  suspended: boolean;
}

export function Workbench({ workspace, reconnecting, suspended }: WorkbenchProps) {
  const pending = useWorkbench((state) => state.pending);
  const jobs = useWorkbench((state) => state.jobs);
  const { project, scenes, profiles, latest, findings } = workspace;
  const { scene, selectScene, profile, selectProfile } = useSelection(project.root, scenes, profiles);
  const sceneLatest = scene ? latest[scene.id] ?? null : null;
  const video = sceneLatest?.video ?? null;
  const { theme, next: nextTheme } = useTheme();
  const diagnosis = useNewestDiagnosis();
  const shownFindings = useMemo(() => [...fromWorkspace(findings), ...(diagnosis?.findings ?? [])], [findings, diagnosis]);
  const findingCount = useMemo(
    () => bySeverity(shownFindings).reduce((count, group) => count + group.cards.length, 0),
    [shownFindings],
  );
  const activity = scene ? stageActivity(jobs, scene.id) : null;
  const selectedProfile = profiles.find((candidate) => candidate.name === profile);

  const [playback] = useState(() => new PlaybackController());
  const [view, setView] = useState<StageView>(() => firstView(sceneLatest));
  const [region, setRegion] = useState<Region>("stage");
  const [tab, setTab] = useState<InspectorTab>("code");
  const [openRequest, setOpenRequest] = useState<OpenRequest | null>(null);
  const [logsJob, setLogsJob] = useState<JobId | null>(null);
  const [trayOpen, setTrayOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const editor = useRef<EditorApi>(null);
  const openSeq = useRef(0);
  const pendingSeek = useRef<{ sceneId: SceneId; seconds: number } | null>(null);
  const loadedScene = useRef<SceneId | null>(null);

  const reveal = useCallback((target: CodeTarget, focus: boolean) => {
    setOpenRequest({ path: target.path, line: target.line, focus, seq: ++openSeq.current });
    if (focus) {
      setTab("code");
      setRegion("inspector");
    }
  }, []);

  useEffect(() => {
    document.title = `${project.name} · Manim Director`;
  }, [project.name]);

  const sceneId = scene?.id ?? null;
  useEffect(() => {
    setView(firstView(sceneId ? latest[sceneId] ?? null : null));
    if (scene) reveal({ path: scene.file, line: scene.span.start }, false);
    // Only a different scene resets the view and scrolls the editor; workspace patches do not.
  }, [sceneId]);

  const videoUrl = video?.artifact.url ?? null;
  useEffect(() => {
    const duration = video?.artifact.media?.duration_seconds ?? null;
    const source = video && duration && isPlayable(video.artifact)
      ? { duration, fps: video.artifact.media?.fps ?? null, marks: video.timeline }
      : null;
    playback.load(source, loadedScene.current === sceneId);
    loadedScene.current = sceneId;
    const seek = pendingSeek.current;
    if (seek && seek.sceneId === sceneId) {
      pendingSeek.current = null;
      playback.seek(seek.seconds);
    }
    // A render is identified by its URL, which carries the file version.
  }, [playback, sceneId, videoUrl]);

  useEffect(() => {
    if (suspended) playback.shuttle(0);
  }, [playback, suspended]);

  const onFinished = ({ action, sceneId: launchedFor }: Launch, job: JobSummary) => {
    if (job.status !== "succeeded") return;
    if (action === "export") setTrayOpen(true);
    if (action === "qa" || action === "diagnose") {
      setLogsJob(null);
      setTrayOpen(false);
      setTab("findings");
      setRegion("inspector");
    }
    const shown = VIEW_AFTER[action];
    if (shown && launchedFor === sceneId) setView(shown);
  };
  const { launch, jobFor, keyOf } = useLaunches(onFinished);

  const context = { scene, latest: sceneLatest, profile, playhead: 0 };
  const stateOf = (action: string, target: SceneId | null = sceneId): ActionState => {
    const job = jobFor(action, target);
    const busy = pending.has(keyOf(action, target)) || (job !== null && isActive(job));
    return { busy, fraction: busy && job ? progressFraction(job.progress) : null };
  };

  const run = async (action: StageAction) => {
    if (!scene) return;
    const readsSource = action === "preview" || action === "render" || action === "still";
    if (readsSource && !(await (editor.current?.saveAll() ?? true))) {
      // The editor now shows the first file it could not save, and its banner says why.
      setTab("code");
      setRegion("inspector");
      return;
    }
    const plan = planAction(action, { ...context, playhead: playback.getSnapshot().time });
    if (plan.ok) await launch(action, sceneId, plan.request);
  };
  const runExport = (format: ExportFormat) => {
    const plan = planExport(format, sceneLatest);
    if (plan.ok) void launch("export", sceneId, plan.request);
  };
  const diagnose = (job: JobSummary) => void launch("diagnose", null, { operation: "diagnose", job_id: job.id });

  /** Transport keys act on the render, so they bring it up. */
  const onVideo = (act: () => void) => () => {
    if (video && view !== "video") setView("video");
    act();
  };
  const showAt = (target: SceneId, seconds: number) => {
    setRegion("stage");
    if (latest[target]?.video) setView("video");
    if (target === sceneId) {
      playback.seek(seconds);
    } else {
      pendingSeek.current = { sceneId: target, seconds };
      selectScene(target);
    }
  };
  const onBeat = (target: Scene, name: string | null, line: number) => {
    reveal({ path: target.file, line }, false);
    const mark = name === null ? undefined : video?.timeline.find((candidate) => candidate.name === name);
    if (mark && isPlayable(video!.artifact)) {
      setView("video");
      setRegion("stage");
      playback.seek(mark.start_seconds);
    } else {
      setTab("code");
      setRegion("inspector");
    }
  };

  useShortcuts(suspended ? {} : {
    toggle_play: onVideo(() => playback.toggle()),
    frame_back: onVideo(() => playback.stepFrames(-1)),
    frame_forward: onVideo(() => playback.stepFrames(1)),
    beat_back: onVideo(() => playback.stepMark(-1)),
    beat_forward: onVideo(() => playback.stepMark(1)),
    reverse: onVideo(() => playback.shuttle(-1)),
    stop: () => playback.shuttle(0),
    forward: onVideo(() => playback.shuttle(1)),
    save: () => void editor.current?.save(),
    preview: () => void run("preview"),
    show_shortcuts: () => setShortcutsOpen(true),
  });

  const regionTab = region === "inspector" ? tab : region;
  const selectRegion = (target: Region | InspectorTab) => {
    if (target === "code" || target === "findings") {
      setTab(target);
      setRegion("inspector");
    } else {
      setRegion(target);
    }
  };

  return (
    <div className="workbench" hidden={suspended}>
      <TopBar
        project={project.name}
        scenes={scenes}
        scene={scene}
        onScene={selectScene}
        profiles={profiles}
        profile={profile}
        onProfile={selectProfile}
        theme={theme}
        onTheme={nextTheme}
        onShortcuts={() => setShortcutsOpen(true)}
        actions={
          <ActionBar
            reason={(action) => {
              const plan = planAction(action, context);
              return plan.ok ? null : plan.reason;
            }}
            state={(action) => stateOf(action)}
            exportReason={(format) => {
              const plan = planExport(format, sceneLatest);
              return plan.ok ? null : plan.reason;
            }}
            exportState={stateOf("export")}
            onRun={(action) => void run(action)}
            onExport={runExport}
          />
        }
        jobs={
          <JobTray
            open={trayOpen}
            onOpenChange={setTrayOpen}
            onLogs={(jobId) => {
              setTrayOpen(false);
              setLogsJob(jobId);
            }}
            onDiagnose={diagnose}
          />
        }
      />
      {reconnecting ? (
        <p className="card banner" data-tone="warning" role="status">
          Reconnecting to the engine; what you see may be out of date.
        </p>
      ) : null}
      <nav className="region-tabs" aria-label="Panels">
        {(["scenes", "stage", "code", "findings"] as const).map((target) => (
          <button key={target} type="button" aria-pressed={regionTab === target} onClick={() => selectRegion(target)}>
            {target[0]!.toUpperCase() + target.slice(1)}
          </button>
        ))}
      </nav>
      <div className="regions">
        <ScenePanel
          scenes={scenes}
          index={workspace.scene_index}
          storyboard={workspace.storyboard}
          latest={latest}
          jobs={jobs}
          selected={scene}
          marks={video?.timeline ?? []}
          playback={playback}
          active={region === "scenes"}
          onSelect={(id) => {
            selectScene(id);
            setRegion("stage");
          }}
          onBeat={onBeat}
        />
        <Stage
          scene={scene}
          latest={sceneLatest}
          aspect={selectedProfile ? selectedProfile.width / selectedProfile.height : 16 / 9}
          activity={activity}
          onLogs={setLogsJob}
          onDiagnose={diagnose}
          view={view}
          onView={setView}
          playback={playback}
          active={region === "stage"}
          onAction={(action) => void run(action)}
          onExportWebm={() => runExport("webm")}
          onSeek={(seconds) => {
            if (sceneId) showAt(sceneId, seconds);
          }}
        />
        <Inspector
          tab={tab}
          onTab={setTab}
          active={region === "inspector"}
          findingCount={findingCount}
          editor={{ request: openRequest, api: editor, onPreview: () => void run("preview") }}
        >
          <FindingsPane
            findings={shownFindings}
            diagnosed={diagnosis?.subject ?? null}
            doctor={workspace.doctor}
            selected={scene}
            onJump={(target) => reveal(target, true)}
            onSeek={showAt}
            onDoctor={() => void launch("doctor", null, { operation: "doctor" })}
            doctorBusy={stateOf("doctor", null).busy}
          />
        </Inspector>
      </div>
      {/* A modal dialog would keep the connection screen inert. */}
      <LogsDialog jobId={suspended ? null : logsJob} onClose={() => setLogsJob(null)} onDiagnose={diagnose} />
      <ShortcutsDialog open={shortcutsOpen && !suspended} onClose={() => setShortcutsOpen(false)} />
    </div>
  );
}
