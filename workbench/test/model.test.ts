import assert from "node:assert/strict";
import test from "node:test";
import type { Artifact, Finding, Scene, SceneLatest, TimelineMark } from "../src/api/types.ts";
import { planAction, planExport } from "../src/model/actions.ts";
import { bySeverity, cleanQa, codeTarget, fromDiagnosis, fromWorkspace } from "../src/model/findings.ts";
import { clockTime, formatTime } from "../src/model/format.ts";
import {
  activityText,
  deliverable,
  expectedSeconds,
  jobTitle,
  progressFraction,
  retryRequest,
  stageActivity,
  statusText,
} from "../src/model/jobs.ts";
import { captionsLabel, downloadUrl, playback } from "../src/model/media.ts";
import { commandFor, type FocusZone, type KeyInput } from "../src/model/shortcuts.ts";
import { lanes, markAt, markStep, rulerTicks, shuttleRate, stepFrames, tickLabel } from "../src/model/timeline.ts";
import { job, progress } from "./fixtures.ts";

const scene: Scene = {
  id: "scenes.py#Recurrence",
  class_name: "Recurrence",
  file: "scenes.py",
  span: { start: 10, end: 40 },
  construct_line: 12,
  bases: ["DirectedScene"],
  theme: null,
  summary: null,
  sections: [],
  beats: [{ name: "hook", span: { start: 14, end: 18 } }],
  declared: null,
  parse_failed: false,
};

function video(overrides: Partial<Artifact["media"] & object> = {}): Artifact {
  return {
    kind: "video",
    path: ".manim-director/artifacts/j1/Recurrence.mp4",
    label: null,
    bytes: 1000,
    media: { container: "mp4", codec: "h264", width: 854, height: 480, fps: 15, duration_seconds: 8.6, has_alpha: false, ...overrides },
    url: "/api/files/.manim-director/artifacts/j1/Recurrence.mp4?v=1-2",
    content_type: "video/mp4",
    version: "1-2",
    scene_id: scene.id,
  };
}

const rendered: SceneLatest = {
  video: { job_id: "j1", finished_at: null, artifact: video(), outdated: false, profile: "draft", timeline: [], captions: [] },
  still: null,
  contact_sheet: null,
};

const marks: TimelineMark[] = [
  { kind: "beat", name: "hook", start_seconds: 0, end_seconds: 3.8, file: "scenes.py", line: 14 },
  { kind: "section", name: "intro", start_seconds: 0, end_seconds: 8.6, file: null, line: null },
  { kind: "beat", name: "log", start_seconds: 3.8, end_seconds: 7.6, file: "scenes.py", line: 20 },
];

test("actions build the contract's requests and say why they cannot run", () => {
  const context = { scene, latest: rendered, profile: "production", playhead: 3.14159 };
  assert.deepEqual(planAction("preview", context), {
    ok: true,
    request: { operation: "render", scene: "Recurrence", file: "scenes.py", profile: "draft" },
  });
  assert.deepEqual(planAction("render", context), {
    ok: true,
    request: { operation: "render", scene: "Recurrence", file: "scenes.py", profile: "production" },
  });
  assert.deepEqual(planAction("frame", { ...context, playhead: 99 }), {
    ok: true,
    request: { operation: "frame", source: { job_id: "j1" }, at_seconds: 8.6 },
  });
  assert.deepEqual(planAction("frame", context), {
    ok: true,
    request: { operation: "frame", source: { job_id: "j1" }, at_seconds: 3.142 },
  });

  const unrendered = { ...context, latest: { video: null, still: null, contact_sheet: null } };
  assert.equal(planAction("contact_sheet", unrendered).ok, false);
  assert.equal(planAction("qa", unrendered).ok, false);
  assert.equal(planAction("still", { ...context, scene: null }).ok, false);
  assert.deepEqual(planExport("zip", null), { ok: true, request: { operation: "export", format: "zip" } });
  assert.deepEqual(planExport("webm", rendered), {
    ok: true,
    request: { operation: "export", format: "webm", source: { job_id: "j1" } },
  });
  assert.equal(planExport("gif", null).ok, false);
});

test("the timeline steps between beats, frames and shuttle speeds", () => {
  assert.deepEqual(lanes(marks).map((lane) => [lane.kind, lane.marks.length]), [["beat", 2], ["section", 1]]);
  assert.equal(markAt(marks, "beat", 3.8)?.name, "log");
  assert.equal(markAt(marks, "beat", 8), null);

  assert.equal(markStep(marks, 0, 1), 3.8);
  assert.equal(markStep(marks, 3.8, 1), null);
  assert.equal(markStep(marks, 3.9, -1), 0, "just past a start goes one beat further back");
  assert.equal(markStep(marks, 5, -1), 3.8);

  assert.equal(stepFrames(0, 1, 10, 2), 0.15);
  assert.equal(stepFrames(0.15, -5, 10, 2), 0.05);
  assert.equal(stepFrames(1.95, 3, 10, 2), 1.95, "never past the last frame");

  assert.equal(shuttleRate(0, 1), 1);
  assert.equal(shuttleRate(1, 1), 2);
  assert.equal(shuttleRate(4, 1), 4);
  assert.equal(shuttleRate(2, -1), -1);
  assert.equal(shuttleRate(-2, 0), 0);
});

test("the ruler labels at most ten whole steps and halves them", () => {
  const short = rulerTicks(8.6);
  assert.deepEqual(short.filter((tick) => tick.labelled).map((tick) => tick.seconds), [1, 2, 3, 4, 5, 6, 7, 8]);
  assert.equal(short[0]?.seconds, 0.5);
  assert.equal(short.length, 17, "nothing at 0 or past the end");
  assert.deepEqual(rulerTicks(38.1).filter((tick) => tick.labelled).map((tick) => tick.seconds), [5, 10, 15, 20, 25, 30, 35]);
  assert.deepEqual(rulerTicks(0), []);
  assert.equal(tickLabel(2.5), "2.5s");
  assert.equal(tickLabel(90), "1:30");
});

test("only browser codecs play; GIFs show as images", () => {
  const latest = (media: Partial<Artifact["media"] & object>) => video(media);
  assert.equal(playback(latest({}), false), "video");
  assert.equal(playback(latest({ container: "gif", codec: "gif" }), false), "image");
  assert.equal(playback(latest({ container: "mov", codec: "prores" }), true), "unplayable");
  assert.equal(playback(latest({ container: "mov", codec: "h264" }), false), "unplayable");
  assert.equal(playback(latest({ container: "mov", codec: "h264" }), true), "video");
  assert.equal(downloadUrl(video()), `${video().url}&download=1`);
});

test("caption files are named by their format", () => {
  assert.equal(captionsLabel({ path: ".manim-director/artifacts/j1/Recurrence.srt" }), "SRT captions");
  assert.equal(captionsLabel({ path: "captions/en.vtt" }), "VTT captions");
});

test("findings group by severity and jump only to project files", () => {
  const finding = { code: "a", message: "m", hint: null, location: null, at_seconds: null, beat: null, frame: null };
  const diagnosed = fromDiagnosis(
    "j9",
    [
      { ...finding, severity: "info" },
      { ...finding, severity: "error", location: { file: "scenes.py", line: 3, column: null } },
    ],
    { sceneId: "scenes.py#A", outdated: false },
  );
  assert.deepEqual(bySeverity(diagnosed).map((group) => group.severity), ["error", "info"]);
  assert.equal(bySeverity(diagnosed)[0]?.cards[0]?.key, "diagnose:j9:1");
  assert.equal(diagnosed[1]?.key, "diagnose:j9:1");
  assert.deepEqual(codeTarget({ file: "scenes.py", line: 3, column: 4 }), { path: "scenes.py", line: 3 });
  assert.equal(codeTarget({ file: "/usr/lib/python3/site.py", line: 3, column: null }), null);
  assert.equal(codeTarget({ file: "C:\\x.py", line: 3, column: null }), null);
  assert.equal(codeTarget(null), null);
});

function qaFinding(id: string, at: number | null, overrides: Partial<Finding> = {}): Finding {
  return {
    id,
    code: "safe_area",
    severity: "warning",
    message: "Content extends outside the safe area.",
    hint: null,
    location: { file: "scenes.py", line: 14, column: null },
    at_seconds: at,
    beat: "hook",
    frame: null,
    source: "qa",
    scene_id: scene.id,
    job_id: "qa1",
    outdated: false,
    frame_url: at === null ? null : `/api/files/f-${at}.png`,
    ...overrides,
  };
}

test("a finding repeated across frames is one card listing its moments", () => {
  const findings = [
    qaFinding("a", 2.5),
    qaFinding("b", 0.5),
    qaFinding("c", 1.5, { location: { file: "scenes.py", line: 20, column: null } }),
    qaFinding("d", null, { code: "blank_frame", severity: "error", message: "Blank." }),
  ];
  const groups = bySeverity(fromWorkspace(findings));
  assert.deepEqual(groups.map((group) => [group.severity, group.cards.length]), [["error", 1], ["warning", 2]]);
  const repeated = groups[1]!.cards[0]!;
  assert.equal(repeated.key, "a");
  assert.deepEqual(repeated.moments, [
    { at_seconds: 0.5, frame_url: "/api/files/f-0.5.png" },
    { at_seconds: 2.5, frame_url: "/api/files/f-2.5.png" },
  ]);
  assert.deepEqual(groups[0]!.cards[0]!.moments, []);
});

test("a passing QA is reported only while it is the scene's newest and found nothing", () => {
  const qa = job({
    id: "qa1",
    sequence: 3,
    operation: "qa",
    status: "succeeded",
    request: { operation: "qa", source: { job_id: "j1" } },
    source_job_id: "j1",
    scene_id: scene.id,
  });
  assert.deepEqual(cleanQa([qa], [], scene.id, rendered), { job: qa, outdated: false });
  assert.equal(cleanQa([qa], fromWorkspace([qaFinding("a", 1)]), scene.id, rendered), null);
  assert.equal(cleanQa([{ ...qa, status: "running" }], [], scene.id, rendered), null);
  assert.equal(cleanQa([qa], [], "scenes.py#Other", rendered), null);

  // It passed on a render that a newer one replaced, or whose scene changed since.
  const rerendered = { ...rendered, video: { ...rendered.video!, job_id: "j2" } };
  assert.equal(cleanQa([qa], [], scene.id, rerendered)?.outdated, true);
  assert.equal(cleanQa([qa], [], scene.id, { ...rendered, video: { ...rendered.video!, outdated: true } })?.outdated, true);
  assert.equal(cleanQa([qa], [], scene.id, null)?.outdated, true);
});

test("the stage reports its scene's newest job while active or failed", () => {
  const sceneId = "scenes/main.py#Recurrence";
  const failed = job({ id: "r1", sequence: 1, status: "failed" });
  assert.equal(stageActivity([failed], sceneId)?.id, "r1");
  assert.equal(activityText(failed), "Render failed");
  const running = job({ id: "s2", sequence: 2, operation: "still", status: "running", profile: "preview" });
  assert.equal(stageActivity([running, failed], sceneId)?.id, "s2");
  assert.equal(activityText(running), "Rendering the last frame · preview");
  assert.equal(stageActivity([{ ...running, status: "succeeded" }, failed], sceneId), null, "a newer success clears the failure");
  const doctor = job({ id: "d3", sequence: 3, operation: "doctor", status: "running", scene_id: null });
  assert.equal(stageActivity([doctor, failed], sceneId)?.id, "r1");
});

test("jobs read as one line and retry only what HTTP accepts", () => {
  assert.equal(jobTitle(job()), "Render Recurrence · draft");
  assert.equal(
    jobTitle(job({ operation: "export", request: { operation: "export", format: "gif", source: { job_id: "j1" } }, profile: null })),
    "Export Recurrence · gif",
  );
  assert.equal(statusText(job({ status: "running", progress: progress(5, "2026-10-07T10:00:00.000Z") })), "Running · animate 50%");
  assert.equal(statusText(job({ status: "running", cancel_requested: true })), "Cancelling");
  const open = { ...progress(4, "2026-10-07T10:00:00.000Z"), total: null, scene_seconds: 10.033 };
  assert.equal(statusText(job({ status: "running", progress: open })), "Running · animate at 0:10.03");
  assert.equal(retryRequest(job({ status: "succeeded" })), null);
  assert.deepEqual(retryRequest(job({ status: "failed" })), job().request);
  assert.equal(
    retryRequest(job({ status: "failed", operation: "ingest", request: { operation: "ingest", sources: [] } })),
    null,
  );
  const zip = { ...video(), kind: "archive" as const, media: null, url: "/api/files/dist/x.zip?v=1" };
  assert.equal(deliverable(job({ operation: "export", status: "succeeded", artifacts: [zip] }))?.url, zip.url);
  assert.equal(deliverable(job({ status: "succeeded", artifacts: [zip] })), null);
});

test("a render without a total measures its progress against the scene's expected length", () => {
  const at = (seconds: number | null) => ({ ...progress(4, "2026-10-07T10:00:00.000Z"), total: null, scene_seconds: seconds });
  assert.equal(progressFraction(progress(5, "2026-10-07T10:00:00.000Z"), 100), 0.5, "a total wins");
  assert.equal(progressFraction(at(2.15), 8.6), 0.25);
  assert.equal(progressFraction(at(12), 8.6), 0.99, "an estimate never reads done");
  assert.equal(progressFraction(at(2.15), null), null);
  assert.equal(progressFraction(at(null), 8.6), null);

  const render = job({ scene_id: scene.id });
  const declared = { ...scene, declared: { id: "recurrence", purpose: null, duration_seconds: 12 } };
  assert.equal(expectedSeconds(render, { scenes: [declared], latest: { [scene.id]: rendered } }), 8.6, "the last render");
  assert.equal(expectedSeconds(render, { scenes: [declared], latest: {} }), 12, "else the declared length");
  assert.equal(expectedSeconds({ ...render, operation: "qa" }, { scenes: [declared], latest: {} }), null);
});

test("keys map to commands only where focus does not need them", () => {
  const key = (k: string, extra: Partial<KeyInput> = {}): KeyInput => ({
    key: k, shiftKey: false, metaKey: false, ctrlKey: false, altKey: false, ...extra,
  });
  const run = (input: KeyInput, zone: FocusZone) => commandFor(input, zone);
  assert.equal(run(key(" "), "page"), "toggle_play");
  assert.equal(run(key(" "), "button"), null);
  assert.equal(run(key("ArrowLeft", { shiftKey: true }), "button"), "beat_back");
  assert.equal(run(key("ArrowRight"), "composite"), null);
  assert.equal(run(key("l"), "editor"), null);
  assert.equal(run(key("L"), "page"), "forward");
  assert.equal(run(key("s", { metaKey: true }), "editor"), "save");
  assert.equal(run(key("Enter", { ctrlKey: true }), "text"), "preview");
  assert.equal(run(key("Escape"), "editor"), null);
  assert.equal(run(key("Escape"), "text"), "close_overlay");
});

test("times read as m:ss.cc", () => {
  assert.equal(formatTime(0), "0:00.00");
  assert.equal(formatTime(63.456), "1:03.46");
  assert.equal(formatTime(59.999), "1:00.00");
  assert.equal(formatTime(Number.NaN), "0:00.00");
});

test("clock times are local, like the rest of the page", (t) => {
  const zone = process.env.TZ;
  t.after(() => {
    if (zone === undefined) delete process.env.TZ;
    else process.env.TZ = zone;
  });
  process.env.TZ = "America/Los_Angeles";
  assert.match(clockTime("2026-10-07T17:19:05.282Z", true), /^10:19:05\b/);
  assert.match(clockTime("2026-10-07T17:19:05.282Z"), /^10:19\b(?!:)/);
});
