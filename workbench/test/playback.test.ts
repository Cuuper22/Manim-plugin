import assert from "node:assert/strict";
import test from "node:test";
import { PlaybackController } from "../src/stage/playback.ts";

const source = { duration: 8, fps: 15, marks: [] };

test("a source the browser cannot play keeps a movable playhead but never runs", () => {
  const playback = new PlaybackController();
  playback.load(source, false);
  playback.cannotPlay();
  playback.toggle();
  playback.shuttle(1);
  playback.shuttle(-1);
  assert.equal(playback.getSnapshot().rate, 0);
  playback.seek(3);
  playback.stepFrames(1);
  assert.ok(playback.getSnapshot().time > 3);

  playback.load({ ...source }, true);
  assert.equal(playback.getSnapshot().playable, true, "a new render may play");
  playback.toggle();
  assert.equal(playback.getSnapshot().rate, 1);
});
