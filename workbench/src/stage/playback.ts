import type { TimelineMark } from "../api/types.ts";
import { markStep, shuttleRate, stepFrames } from "../model/timeline.ts";

export interface PlaybackSource {
  duration: number;
  /** `null` when the probe did not know; frame steps then assume 30 fps. */
  fps: number | null;
  marks: readonly TimelineMark[];
}

export interface PlaybackState {
  source: PlaybackSource | null;
  time: number;
  /** 0 while paused; negative plays backwards. */
  rate: number;
  /** `false` once the browser could not play the source: the playhead still moves by hand, but does not run. */
  playable: boolean;
}

const FALLBACK_FPS = 30;

/**
 * The playhead. It exists without a `<video>` (the stage may show a still),
 * drives one when attached, and plays backwards by seeking each animation
 * frame because browsers reject negative playback rates.
 */
export class PlaybackController {
  readonly #listeners = new Set<() => void>();
  #video: HTMLVideoElement | null = null;
  #state: PlaybackState = { source: null, time: 0, rate: 0, playable: true };
  #frame: number | null = null;
  /** Bumped per run; a late `play()` rejection of an older run is ignored. */
  #run = 0;

  readonly getSnapshot = (): PlaybackState => this.#state;

  readonly subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** A newer render of the same scene keeps the playhead (`keepTime`); another scene starts over. */
  load(source: PlaybackSource | null, keepTime: boolean): void {
    this.#halt();
    const time = keepTime && source ? Math.min(this.#state.time, source.duration) : 0;
    this.#set({ source, time, rate: 0, playable: true });
  }

  /** The browser cannot decode or load the source; until the next `load`, play does nothing. */
  cannotPlay(): void {
    this.#halt();
    this.#set({ ...this.#state, rate: 0, playable: false });
  }

  /** Ref callback for the stage's `<video>`. */
  readonly attach = (video: HTMLVideoElement | null): void => {
    if (this.#video === video) return;
    this.#halt();
    this.#video?.removeEventListener("ended", this.#ended);
    this.#video = video;
    if (!video) {
      this.#set({ ...this.#state, rate: 0 });
      return;
    }
    video.addEventListener("ended", this.#ended);
    video.currentTime = this.#state.time;
    // Play pressed while another view was showing starts once the video is here.
    if (this.#state.rate !== 0) this.#start(video, this.#state.rate);
  };

  seek(seconds: number): void {
    const { source } = this.#state;
    if (!source) return;
    const time = Math.min(Math.max(seconds, 0), source.duration);
    if (this.#video) this.#video.currentTime = time;
    this.#set({ ...this.#state, time });
  }

  toggle(): void {
    this.#play(this.#state.rate === 0 ? 1 : 0);
  }

  /** J (-1), K (0), L (1). */
  shuttle(direction: -1 | 0 | 1): void {
    this.#play(shuttleRate(this.#state.rate, direction));
  }

  stepFrames(frames: number): void {
    const { source } = this.#state;
    if (!source) return;
    this.#play(0);
    this.seek(stepFrames(this.#state.time, frames, source.fps ?? FALLBACK_FPS, source.duration));
  }

  /** To the previous or next beat; to either end when there is none. */
  stepMark(direction: 1 | -1): void {
    const { source } = this.#state;
    if (!source) return;
    this.seek(markStep(source.marks, this.#state.time, direction) ?? (direction === 1 ? source.duration : 0));
  }

  #play(rate: number): void {
    const { source, playable } = this.#state;
    if (!source || (rate !== 0 && !playable)) return;
    const wasPlaying = this.#state.rate > 0 && this.#video !== null;
    this.#halt();
    // Forward play updates the playhead once per animation frame; the video knows exactly where it stopped.
    let time = wasPlaying ? this.#video!.currentTime : this.#state.time;
    if (rate > 0 && time >= source.duration - 1e-3) time = 0;
    if (rate < 0 && time <= 1e-3) rate = 0;
    if (this.#video) this.#video.currentTime = time;
    this.#set({ ...this.#state, time, rate });
    if (this.#video && rate !== 0) this.#start(this.#video, rate);
  }

  #start(video: HTMLVideoElement, rate: number): void {
    const run = ++this.#run;
    if (rate > 0) {
      video.playbackRate = rate;
      video.play().catch(() => {
        if (run === this.#run) this.#set({ ...this.#state, rate: 0 });
      });
      const follow = () => {
        this.#set({ ...this.#state, time: video.currentTime });
        this.#frame = requestAnimationFrame(follow);
      };
      this.#frame = requestAnimationFrame(follow);
      return;
    }
    video.pause();
    let last = performance.now();
    const rewind = (now: number) => {
      const time = Math.max(0, this.#state.time + (rate * (now - last)) / 1000);
      last = now;
      video.currentTime = time;
      this.#set({ ...this.#state, time, rate: time === 0 ? 0 : rate });
      this.#frame = time === 0 ? null : requestAnimationFrame(rewind);
    };
    this.#frame = requestAnimationFrame(rewind);
  }

  #halt(): void {
    this.#run += 1;
    if (this.#frame !== null) cancelAnimationFrame(this.#frame);
    this.#frame = null;
    this.#video?.pause();
  }

  readonly #ended = (): void => {
    this.#halt();
    const { source } = this.#state;
    if (source) this.#set({ ...this.#state, time: source.duration, rate: 0 });
  };

  #set(state: PlaybackState): void {
    const current = this.#state;
    const same = state.source === current.source && state.time === current.time && state.rate === current.rate;
    if (same && state.playable === current.playable) return;
    this.#state = state;
    for (const listener of this.#listeners) listener();
  }
}
