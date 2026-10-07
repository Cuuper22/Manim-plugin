import { useState, type CSSProperties, type ReactNode } from "react";
import type { Artifact, JobId, JobSummary, Scene, SceneLatest } from "../api/types.ts";
import type { StageAction } from "../model/actions.ts";
import { formatTime } from "../model/format.ts";
import { activityText, isActive, progressDetail } from "../model/jobs.ts";
import { captionsLabel, downloadUrl, mediaSummary, playback as playbackOf } from "../model/media.ts";
import type { PlaybackController } from "../stage/playback.ts";
import { useStore } from "../store/useWorkbench.ts";
import { Icon } from "./Icon.tsx";
import { JobProgress } from "./Progress.tsx";
import { TabList, panelId, tabId, type TabSpec } from "./TabList.tsx";
import { Timeline } from "./Timeline.tsx";

export type StageView = "video" | "still" | "sheet";

interface StageProps {
  scene: Scene | null;
  latest: SceneLatest | null;
  /** Width over height of the selected profile, for the empty canvas. */
  aspect: number;
  /** The scene's running or failed job, from `stageActivity`. */
  activity: JobSummary | null;
  view: StageView;
  onView: (view: StageView) => void;
  playback: PlaybackController;
  active: boolean;
  onAction: (action: StageAction) => void;
  onExportWebm: () => void;
  /** Shows the video at `seconds`. */
  onSeek: (seconds: number) => void;
  onLogs: (jobId: JobId) => void;
  onDiagnose: (job: JobSummary) => void;
}

const CAN_PLAY_QUICKTIME = document.createElement("video").canPlayType("video/quicktime") !== "";

const VIEWS: readonly TabSpec<StageView>[] = [
  { id: "video", label: "Render" },
  { id: "still", label: "Still" },
  { id: "sheet", label: "Contact sheet" },
];

/** Whether the stage plays this render as seekable video. */
export function isPlayable(artifact: Artifact): boolean {
  return playbackOf(artifact, CAN_PLAY_QUICKTIME) === "video";
}

/** The selected scene's newest render, still or contact sheet. */
export function Stage(props: StageProps) {
  const { scene, latest, aspect, activity, view, onView, playback, active, onAction, onSeek } = props;
  const video = latest?.video ?? null;
  const rendering = activity !== null && isActive(activity) && activity.operation === "render";

  let content: ReactNode;
  if (!scene) {
    content = <Canvas aspect={aspect} text="Select a scene to see its renders." />;
  } else if (view === "video") {
    content = video ? (
      <VideoView
        key={video.artifact.url}
        artifact={video.artifact}
        still={latest?.still ?? null}
        playback={playback}
        onExportWebm={props.onExportWebm}
      />
    ) : rendering ? (
      <Canvas aspect={aspect} text={`Rendering ${scene.class_name}…`} />
    ) : (
      <Canvas aspect={aspect} text={`${scene.class_name} has no render yet.`}>
        <button type="button" className="primary" onClick={() => onAction("preview")}>
          <Icon name="play" />
          Preview
        </button>
      </Canvas>
    );
  } else if (view === "still") {
    content = latest?.still ? (
      <img className="media" src={latest.still.artifact.url} alt={stillCaption(scene, latest)} />
    ) : (
      <Canvas aspect={aspect} text="No still yet. Still renders the last frame; Frame at playhead grabs one from the render.">
        <button type="button" onClick={() => onAction("still")}>
          <Icon name="still" />
          Still
        </button>
      </Canvas>
    );
  } else {
    content = latest?.contact_sheet ? (
      <ContactSheet scene={scene} latest={latest} onSeek={onSeek} />
    ) : (
      <Canvas aspect={aspect} text="No contact sheet yet. It lays frames of the render out side by side.">
        {video ? (
          <button type="button" onClick={() => onAction("contact_sheet")}>
            <Icon name="sheet" />
            Contact sheet
          </button>
        ) : (
          <button type="button" className="primary" onClick={() => onAction("preview")}>
            <Icon name="play" />
            Preview first
          </button>
        )}
      </Canvas>
    );
  }

  return (
    <main className="region region-stage" id="region-stage" aria-label="Stage" data-active={active || undefined}>
      <header className="bar stage-header">
        <TabList label="Stage view" idPrefix="stage" tabs={VIEWS} selected={view} onSelect={onView} />
        <StageMeta latest={latest} view={view} />
      </header>
      {activity ? (
        <Activity key={activity.id} job={activity} onLogs={props.onLogs} onDiagnose={props.onDiagnose} />
      ) : null}
      <div className="stage" id={panelId("stage", view)} role="tabpanel" aria-labelledby={tabId("stage", view)}>
        {content}
      </div>
      {video && isPlayable(video.artifact) ? <Timeline playback={playback} /> : null}
    </main>
  );
}

/** The frame a render will fill, at the selected profile's aspect ratio. */
function Canvas({ aspect, text, children }: { aspect: number; text: string; children?: ReactNode }) {
  return (
    <div className="canvas" style={{ "--aspect": aspect } as CSSProperties}>
      <p>{text}</p>
      {children}
    </div>
  );
}

interface ActivityProps {
  job: JobSummary;
  onLogs: (jobId: JobId) => void;
  onDiagnose: (job: JobSummary) => void;
}

/** The scene's job in progress, or the reason its last one failed. */
function Activity({ job, onLogs, onDiagnose }: ActivityProps) {
  const store = useStore();
  const [dismissed, setDismissed] = useState(false);
  if (dismissed) return null;
  const failed = job.status === "failed";
  return (
    // Not a live region: progress changes several times a second; the job tray announces starts and ends.
    <div className="activity" data-tone={failed ? "danger" : undefined}>
      {failed ? <Icon name="alert" /> : null}
      <p className="activity-text">
        <span className="activity-title">{activityText(job)}</span>
        <span className="muted">{failed ? job.error?.message ?? "" : activityDetail(job)}</span>
      </p>
      <button type="button" className="quiet small" onClick={() => onLogs(job.id)}>
        Logs
      </button>
      {failed ? (
        <>
          {job.operation !== "diagnose" ? (
            <button type="button" className="quiet small" onClick={() => onDiagnose(job)}>
              Diagnose
            </button>
          ) : null}
          <button type="button" className="quiet small icon" aria-label="Dismiss" onClick={() => setDismissed(true)}>
            <Icon name="close" />
          </button>
        </>
      ) : (
        <button type="button" className="quiet small" disabled={job.cancel_requested} onClick={() => void store.cancel(job.id)}>
          Cancel
        </button>
      )}
      {failed ? null : <JobProgress job={job} label={`${activityText(job)} progress`} />}
    </div>
  );
}

/** What the shown artifact is, whether its scene changed since, and a render's caption files. */
function StageMeta({ latest, view }: { latest: SceneLatest | null; view: StageView }) {
  let shown: { artifact: Artifact; outdated: boolean } | null = null;
  let text = "";
  let captions: readonly Artifact[] = [];
  if (view === "video" && latest?.video) {
    shown = latest.video;
    text = [latest.video.profile, mediaSummary(shown.artifact)].filter(Boolean).join(" · ");
    captions = latest.video.captions;
  } else if (view === "still" && latest?.still) {
    shown = latest.still;
    text = stillLabel(latest.still.at_seconds);
  } else if (view === "sheet" && latest?.contact_sheet) {
    shown = latest.contact_sheet;
    text = `${latest.contact_sheet.frames.length} frames · ${mediaSummary(shown.artifact)}`;
  }
  if (!shown) return null;
  return (
    <p className="row meta stage-meta">
      <span>{text}</span>
      {shown.outdated ? <Outdated /> : null}
      {captions.map((file) => (
        <a key={file.path} className="button quiet small" href={downloadUrl(file)} download>
          <Icon name="export" />
          {captionsLabel(file)}
        </a>
      ))}
    </p>
  );
}

function Outdated() {
  return (
    <span className="tag" data-tone="warning" title="The scene's source changed after this was made.">
      Outdated
    </span>
  );
}

interface VideoViewProps {
  artifact: Artifact;
  still: SceneLatest["still"];
  playback: PlaybackController;
  onExportWebm: () => void;
}

function VideoView({ artifact, still, playback, onExportWebm }: VideoViewProps) {
  const store = useStore();
  const [failure, setFailure] = useState<"decode" | "load" | null>(null);
  const mode = playbackOf(artifact, CAN_PLAY_QUICKTIME);
  const format = `${artifact.media?.container ?? "unknown"}, ${artifact.media?.codec ?? "unknown codec"}`;

  if (mode === "image") return <img className="media" src={artifact.url} alt="The latest render (GIF)" />;
  if (mode === "unplayable" || failure === "decode") {
    return (
      <div className="stage-fallback">
        {still ? (
          <figure>
            <img className="media" src={still.artifact.url} alt="The newest still of this scene" />
            <figcaption className="row meta">
              <span>Newest still · {stillLabel(still.at_seconds)}</span>
              {still.outdated ? <Outdated /> : null}
            </figcaption>
          </figure>
        ) : null}
        <p>
          {mode === "unplayable"
            ? `This render (${format}) does not play in browsers.`
            : `This browser cannot decode this render (${format}).`}{" "}
          WebM plays everywhere.
        </p>
        <button type="button" onClick={onExportWebm}>
          <Icon name="export" />
          Export as WebM
        </button>
      </div>
    );
  }
  if (failure === "load") return <p className="pane-note">The video could not be loaded.</p>;

  const onError = async () => {
    // A <video> cannot see HTTP statuses; ask once whether the file was replaced (410) or cannot be decoded.
    const response = await fetch(artifact.url, { method: "HEAD" }).catch(() => null);
    if (response?.status === 410) return store.refresh();
    playback.cannotPlay();
    setFailure(response?.ok ? "decode" : "load");
  };

  return (
    <video
      ref={playback.attach}
      className="media"
      src={artifact.url}
      preload="auto"
      playsInline
      aria-label="The latest render"
      onError={() => void onError()}
    />
  );
}

interface ContactSheetProps {
  scene: Scene;
  latest: SceneLatest;
  onSeek: (seconds: number) => void;
}

function ContactSheet({ scene, latest, onSeek }: ContactSheetProps) {
  const sheet = latest.contact_sheet!;
  return (
    <figure className="sheet">
      <img className="media" src={sheet.artifact.url} alt={`Contact sheet of ${scene.class_name}`} />
      <figcaption>
        <ul className="row moments" aria-label="Frames on the sheet">
          {sheet.frames.map((frame, index) => (
            <li key={index} className="moment">
              <button
                type="button"
                className="mono"
                disabled={!latest.video}
                title="Show the render here"
                onClick={() => onSeek(frame.at_seconds)}
              >
                {formatTime(frame.at_seconds)}
                {frame.beat ? <span className="muted"> {frame.beat}</span> : null}
              </button>
            </li>
          ))}
        </ul>
      </figcaption>
    </figure>
  );
}

function activityDetail(job: JobSummary): string {
  if (job.cancel_requested) return "Cancelling";
  if (job.status === "queued") return "Queued";
  return job.progress ? progressDetail(job.progress) : "Starting";
}

function stillLabel(at: number | null): string {
  return at === null ? "Last frame" : `Frame at ${formatTime(at)}`;
}

function stillCaption(scene: Scene, latest: SceneLatest): string {
  const at = latest.still?.at_seconds ?? null;
  return at === null ? `Last frame of ${scene.class_name}` : `${scene.class_name} at ${formatTime(at)}`;
}
