import { useState } from "react";
import type { Artifact, Scene, SceneLatest } from "../api/types.ts";
import type { StageAction } from "../model/actions.ts";
import { formatTime } from "../model/format.ts";
import { mediaSummary, playback as playbackOf } from "../model/media.ts";
import type { PlaybackController } from "../stage/playback.ts";
import { useStore } from "../store/useWorkbench.ts";
import { TabList, panelId, tabId, type TabSpec } from "./TabList.tsx";
import { Timeline } from "./Timeline.tsx";

export type StageView = "video" | "still" | "sheet";

interface StageProps {
  scene: Scene | null;
  latest: SceneLatest | null;
  view: StageView;
  onView: (view: StageView) => void;
  playback: PlaybackController;
  active: boolean;
  onAction: (action: StageAction) => void;
  onExportWebm: () => void;
  /** Shows the video at `seconds`. */
  onSeek: (seconds: number) => void;
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
export function Stage({ scene, latest, view, onView, playback, active, onAction, onExportWebm, onSeek }: StageProps) {
  const video = latest?.video ?? null;

  return (
    <main className="region region-stage" id="region-stage" aria-label="Stage" data-active={active || undefined}>
      <header className="bar stage-header">
        <TabList label="Stage view" idPrefix="stage" tabs={VIEWS} selected={view} onSelect={onView} />
        <StageMeta latest={latest} view={view} />
      </header>
      <div className="stage" id={panelId("stage", view)} role="tabpanel" aria-labelledby={tabId("stage", view)}>
        {!scene ? (
          <p className="pane-note">Select a scene to see its renders.</p>
        ) : view === "video" ? (
          video ? (
            <VideoView
              key={video.artifact.url}
              artifact={video.artifact}
              still={latest?.still?.artifact ?? null}
              playback={playback}
              onExportWebm={onExportWebm}
            />
          ) : (
            <Empty text={`${scene.class_name} has no render yet.`} action="preview" label="Preview" onAction={onAction} />
          )
        ) : view === "still" ? (
          latest?.still ? (
            <img className="media" src={latest.still.artifact.url} alt={stillCaption(scene, latest)} />
          ) : (
            <Empty
              text="No still yet: Still renders the last frame, Frame at playhead grabs one from the render."
              action="still"
              label="Still"
              onAction={onAction}
            />
          )
        ) : latest?.contact_sheet ? (
          <ContactSheet scene={scene} latest={latest} onSeek={onSeek} />
        ) : (
          <Empty
            text="No contact sheet yet: it lays frames of the render out side by side."
            action={video ? "contact_sheet" : "preview"}
            label={video ? "Contact sheet" : "Preview"}
            onAction={onAction}
          />
        )}
      </div>
      {video && isPlayable(video.artifact) ? <Timeline playback={playback} /> : null}
    </main>
  );
}

/** What the shown artifact is, and whether its scene changed since. */
function StageMeta({ latest, view }: { latest: SceneLatest | null; view: StageView }) {
  let shown: { artifact: Artifact; outdated: boolean } | null = null;
  let text = "";
  if (view === "video" && latest?.video) {
    shown = latest.video;
    text = [latest.video.profile, mediaSummary(shown.artifact)].filter(Boolean).join(" · ");
  } else if (view === "still" && latest?.still) {
    shown = latest.still;
    text = latest.still.at_seconds === null ? "Last frame" : `Frame at ${formatTime(latest.still.at_seconds)}`;
  } else if (view === "sheet" && latest?.contact_sheet) {
    shown = latest.contact_sheet;
    text = `${latest.contact_sheet.frames.length} frames · ${mediaSummary(shown.artifact)}`;
  }
  if (!shown) return null;
  return (
    <p className="row meta stage-meta">
      <span>{text}</span>
      {shown.outdated ? (
        <span className="warning" title="The scene's source changed after this was made.">
          Outdated
        </span>
      ) : null}
    </p>
  );
}

interface VideoViewProps {
  artifact: Artifact;
  still: Artifact | null;
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
      <div className="stage-empty">
        {still ? <img className="media" src={still.url} alt="The newest still of this scene" /> : null}
        <p>
          {mode === "unplayable"
            ? `This render (${format}) does not play in browsers.`
            : `This browser cannot decode this render (${format}).`}{" "}
          WebM plays everywhere.
        </p>
        <button type="button" onClick={onExportWebm}>
          Export as WebM
        </button>
      </div>
    );
  }
  if (failure === "load") return <p className="pane-note">The video could not be loaded.</p>;

  const onError = async () => {
    // A <video> cannot see HTTP statuses; ask once whether the file was replaced (410) or cannot be decoded.
    const response = await fetch(artifact.url, { method: "HEAD" }).catch(() => null);
    if (response?.status === 410) store.refresh();
    else setFailure(response?.ok ? "decode" : "load");
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
        <ul aria-label="Frames on the sheet">
          {sheet.frames.map((frame, index) => (
            <li key={index}>
              <button type="button" className="small mono" disabled={!latest.video} onClick={() => onSeek(frame.at_seconds)}>
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

interface EmptyProps {
  text: string;
  action: StageAction;
  label: string;
  onAction: (action: StageAction) => void;
}

function Empty({ text, action, label, onAction }: EmptyProps) {
  return (
    <div className="stage-empty">
      <p>{text}</p>
      <button type="button" className="primary" onClick={() => onAction(action)}>
        {label}
      </button>
    </div>
  );
}

function stillCaption(scene: Scene, latest: SceneLatest): string {
  const at = latest.still?.at_seconds ?? null;
  return at === null ? `Last frame of ${scene.class_name}` : `${scene.class_name} at ${formatTime(at)}`;
}
