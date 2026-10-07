import type { Artifact } from "../api/types.ts";

export type Playback = "video" | "image" | "unplayable";

const VIDEO_CONTAINERS = new Set(["mp4", "webm", "mov"]);
const BROWSER_CODECS = new Set(["h264", "vp9", "av1"]);

/**
 * How the stage shows a render (CONTRACT-http §9): GIFs as images, browser
 * codecs as seekable video, anything else (e.g. ProRes with alpha) not at all.
 */
export function playback(artifact: Artifact, canPlayQuickTime: boolean): Playback {
  const media = artifact.media;
  if (!media) return "unplayable";
  if (media.container === "gif") return "image";
  const playable = VIDEO_CONTAINERS.has(media.container)
    && media.codec !== null
    && BROWSER_CODECS.has(media.codec)
    && (media.container !== "mov" || canPlayQuickTime);
  return playable ? "video" : "unplayable";
}

/** `854×480 · 15 fps · 8.6 s` from whatever the probe knew. */
export function mediaSummary(artifact: Artifact): string {
  const media = artifact.media;
  if (!media) return artifact.kind;
  const parts = [`${media.width}×${media.height}`];
  if (media.fps !== null) parts.push(`${Number(media.fps.toFixed(2))} fps`);
  if (media.duration_seconds !== null) parts.push(`${Number(media.duration_seconds.toFixed(2))} s`);
  return parts.join(" · ");
}

/** The artifact URL as a download (`Content-Disposition: attachment`). */
export function downloadUrl(artifact: Pick<Artifact, "url">): string {
  return `${artifact.url}${artifact.url.includes("?") ? "&" : "?"}download=1`;
}

/** `SRT captions` for `Recurrence.srt`. */
export function captionsLabel(artifact: Pick<Artifact, "path">): string {
  const extension = artifact.path.slice(artifact.path.lastIndexOf(".") + 1);
  return `${extension.toUpperCase()} captions`;
}
