import { useSyncExternalStore } from "react";
import type { JobSummary, Scene, SceneId, SceneIndexStatus, SceneLatest, StoryboardBeat, TimelineMark } from "../api/types.ts";
import { formatSeconds } from "../model/format.ts";
import { activityText, isActive, progressDetail, stageActivity } from "../model/jobs.ts";
import { markAt } from "../model/timeline.ts";
import type { PlaybackController } from "../stage/playback.ts";

interface ScenePanelProps {
  scenes: readonly Scene[];
  index: SceneIndexStatus;
  storyboard: readonly StoryboardBeat[];
  latest: Readonly<Record<SceneId, SceneLatest>>;
  jobs: readonly JobSummary[];
  selected: Scene | null;
  /** The selected scene's render marks, for the beat under the playhead. */
  marks: readonly TimelineMark[];
  playback: PlaybackController;
  active: boolean;
  onSelect: (id: SceneId) => void;
  onBeat: (scene: Scene, name: string | null, line: number) => void;
}

/** The scenes found in the source, the selected one opened up into its beats and sections. */
export function ScenePanel(props: ScenePanelProps) {
  const { scenes, index, storyboard, selected } = props;
  const unplaced = storyboard.filter((beat) => beat.code === null);
  return (
    <nav className="region region-scenes" id="region-scenes" aria-label="Scenes" data-active={props.active || undefined}>
      <header className="bar">
        <h2>Scenes</h2>
        <span className="meta">{indexNote(index, scenes.length)}</span>
      </header>
      {scenes.length === 0 && index.state === "indexing" ? (
        <SkeletonRows />
      ) : scenes.length === 0 ? (
        <p className="pane-note">{emptyNote(index)}</p>
      ) : (
        <ul>
          {scenes.map((scene) => (
            <li key={scene.id}>
              <button
                type="button"
                className="scene"
                aria-current={scene.id === selected?.id || undefined}
                onClick={() => props.onSelect(scene.id)}
              >
                <span className="scene-name">{scene.class_name}</span>
                <span className="meta mono">
                  {scene.file}:{scene.span.start}–{scene.span.end}
                </span>
                {scene.summary ? <span className="meta clamp">{scene.summary}</span> : null}
                <SceneStatus scene={scene} latest={props.latest[scene.id] ?? null} jobs={props.jobs} />
              </button>
              {scene.id === selected?.id ? <SceneOutline {...props} scene={scene} /> : null}
            </li>
          ))}
        </ul>
      )}
      {unplaced.length > 0 ? (
        <section className="unplaced" aria-labelledby="unplaced-heading">
          <h3 id="unplaced-heading" className="meta">Storyboard beats not in code</h3>
          <ul>
            {unplaced.map((beat) => (
              <li key={beat.id}>
                <span className="beat-name">{beat.id}</span>
                {beat.takeaway ? <span className="meta">{beat.takeaway}</span> : null}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </nav>
  );
}

type StatusState = "none" | "running" | "rendered" | "outdated" | "failed";

function SceneStatus({ scene, latest, jobs }: { scene: Scene; latest: SceneLatest | null; jobs: readonly JobSummary[] }) {
  if (scene.parse_failed) return <Status state="failed">Does not parse; showing its last good version</Status>;
  const activity = stageActivity(jobs, scene.id);
  if (activity && isActive(activity)) {
    const detail = activity.progress ? ` · ${progressDetail(activity.progress)}` : "…";
    return <Status state="running">{`${activityText(activity)}${detail}`}</Status>;
  }
  if (activity) return <Status state="failed">{activityText(activity)}</Status>;
  const video = latest?.video;
  if (!video) return <Status state="none">Not rendered</Status>;
  const rendered = video.profile ? `${video.profile} render` : "Rendered";
  return <Status state={video.outdated ? "outdated" : "rendered"}>{video.outdated ? `${rendered} · outdated` : rendered}</Status>;
}

function Status({ state, children }: { state: StatusState; children: string }) {
  return (
    <span className="meta status" data-state={state}>
      {children}
    </span>
  );
}

/** Stand-ins for the scene list while the first scan runs. */
function SkeletonRows() {
  return (
    <ul className="skeleton" aria-busy="true" aria-label="Looking for scenes">
      {[60, 44, 52].map((width) => (
        <li key={width}>
          <span style={{ width: `${width}%` }} />
          <span style={{ width: `${width + 24}%` }} />
        </li>
      ))}
    </ul>
  );
}

function SceneOutline({ scene, storyboard, marks, playback, onBeat }: ScenePanelProps & { scene: Scene }) {
  const current = useSyncExternalStore(playback.subscribe, () => markAt(marks, "beat", playback.getSnapshot().time)?.name ?? null);
  if (scene.beats.length === 0 && scene.sections.length === 0) return null;
  return (
    <div className="outline">
      {scene.beats.length > 0 ? (
        <ul aria-label={`Beats of ${scene.class_name}`}>
          {scene.beats.map((beat) => {
            const story = storyboard.find((entry) => entry.code?.scene_id === scene.id && entry.code.line === beat.span.start);
            const mark = marks.find((candidate) => candidate.kind === "beat" && candidate.name === beat.name);
            const seconds = mark ? mark.end_seconds - mark.start_seconds : story?.duration_seconds ?? null;
            return (
              <li key={`${beat.name}:${beat.span.start}`}>
                <button
                  type="button"
                  className="beat"
                  aria-current={beat.name !== null && beat.name === current ? "time" : undefined}
                  onClick={() => onBeat(scene, beat.name, beat.span.start)}
                >
                  <span className="beat-name">{beat.name ?? "unnamed beat"}</span>
                  <span className="meta mono">
                    {seconds !== null ? `${formatSeconds(seconds)} · ` : ""}line {beat.span.start}
                  </span>
                  {story?.takeaway ? <span className="meta">{story.takeaway}</span> : null}
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
      {scene.sections.length > 0 ? (
        <ul aria-label={`Sections of ${scene.class_name}`}>
          {scene.sections.map((section) => (
            <li key={`${section.name}:${section.line}`}>
              <button type="button" className="beat" onClick={() => onBeat(scene, section.name, section.line)}>
                <span className="beat-name">{section.name ?? "unnamed section"}</span>
                <span className="meta mono">section · line {section.line}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function indexNote(index: SceneIndexStatus, count: number): string {
  if (index.state === "indexing") return "Indexing…";
  if (index.state === "failed") return "Index failed";
  return index.truncated ? `${count}, first ${index.files} files` : String(count);
}

function emptyNote(index: SceneIndexStatus): string {
  if (index.state === "failed") return `Scenes could not be read: ${index.error?.message ?? "unknown error"}`;
  return "No scenes yet. A scene is a Python class that derives from a Manim Scene, e.g. DirectedScene.";
}
