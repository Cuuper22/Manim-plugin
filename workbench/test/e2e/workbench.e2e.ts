// The core flow against a real engine and runtime, in Chromium: npm run test:e2e.
// Skipped without an engine binary (cargo build, or MANIM_DIRECTOR_BIN) or a
// Playwright Chromium (PLAYWRIGHT_BROWSERS_PATH), and when the engine's doctor
// says this machine cannot render. The engine finds Python and the runtime as
// it always does (e.g. MANIM_DIRECTOR_PYTHON).
import assert from "node:assert/strict";
import { appendFileSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { chromium, type Locator, type Page } from "playwright-core";
import type { SourcePage } from "../../src/api/types.ts";
import { Engine, engineBinary } from "./engine.ts";

const RENDER_MS = 240_000;
const STEP_MS = 60_000;

const binary = engineBinary();
const skip = binary === null
  ? "no engine binary: run cargo build, or point MANIM_DIRECTOR_BIN at one"
  : existsSync(chromium.executablePath())
    ? false
    : "no Playwright Chromium: set PLAYWRIGHT_BROWSERS_PATH";

test("preview, inspect, check, edit, cancel and export a scene", { skip, timeout: 15 * 60_000 }, async (t) => {
  const engine = await Engine.start(binary!);
  t.after(() => engine.stop());

  const doctor = await engine.doctor(STEP_MS);
  if (!doctor?.report.capabilities.render || !doctor.report.capabilities.video_tools) {
    t.skip(`this machine cannot render: ${JSON.stringify(doctor?.report.capabilities ?? "no doctor report")}`);
    return;
  }
  // Playwright's Chromium cannot decode H.264, so playback is checked on a WebM profile.
  const spec = await engine.api<SourcePage>("/api/source?path=director.yaml&start_line=1&end_line=1");
  await engine.api("/api/source", {
    method: "PUT",
    body: {
      path: "director.yaml",
      expected_revision: spec.revision,
      edit: { kind: "merge_patch", patch: { profiles: { web: { quality: "low", format: "webm" } } } },
    },
  });

  const browser = await chromium.launch();
  t.after(() => browser.close());
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const problems: string[] = [];
  /** Requests the test cuts off on purpose fail loudly in the console. */
  let outage = false;
  page.on("pageerror", (error) => problems.push(error.message));
  page.on("console", (message) => {
    // A save that loses a revision race is answered 409, which the browser logs.
    if (message.type() === "error" && !outage && !message.text().includes("status of 409")) problems.push(message.text());
  });

  await page.goto(engine.url);
  await page.locator(".workbench").waitFor();
  const scenes = (await engine.state()).scenes;
  const sequenceData = scenes.find((scene) => scene.class_name === "SequenceData");
  assert.ok(sequenceData, "the example has a SequenceData scene");
  const actions = page.getByRole("group", { name: "Scene actions" });
  const action = (name: string) => actions.getByRole("button", { name, exact: true });
  const sceneButton = (name: string) => page.getByRole("navigation", { name: "Scenes" }).getByRole("button", { name: new RegExp(`^${name}`) });

  await t.test("selecting a scene opens its source at the class", async () => {
    await sceneButton("SequenceData").click();
    await until(page, () => activeLine(page).then((line) => line === sequenceData.span.start), "editor at the class line");
  });

  await t.test("an edit saves to disk", async () => {
    await page.locator(".cm-line", { hasText: "self.place(linear)" }).click();
    await page.keyboard.press("End");
    await page.keyboard.press("Enter");
    // A wide bar at the frame's right edge leaves the safe area, which QA reports below.
    await page.keyboard.type("self.add(Rectangle(width=2, height=4, fill_opacity=1, color=YELLOW).to_edge(RIGHT, buff=0))");
    await page.keyboard.press("Control+s");
    await until(page, async () => (await page.locator(".code > .bar").innerText()).includes("Saved"), "saved");
    assert.match(readFileSync(join(engine.root, "scenes.py"), "utf8"), /color=YELLOW\)\.to_edge\(RIGHT, buff=0\)/);
  });

  await t.test("a render plays and seeks", async () => {
    await page.getByLabel("Render profile").selectOption("web");
    await action("Render").click();
    const video = page.locator("video");
    await video.waitFor({ timeout: RENDER_MS });
    await page.waitForFunction(() => (document.querySelector("video")?.readyState ?? 0) >= 2, null, { timeout: STEP_MS });
    assert.match(await page.locator(".stage-meta").innerText(), /^web · 854×480/);

    await page.getByRole("button", { name: "Play", exact: true }).click();
    await until(page, () => video.evaluate((element: HTMLVideoElement) => element.currentTime > 0.5), "playing");
    await page.keyboard.press("k");
    assert.equal(await video.evaluate((element: HTMLVideoElement) => element.paused), true);

    const duration = await video.evaluate((element: HTMLVideoElement) => element.duration);
    await clickAt(page.getByRole("slider", { name: "Playhead" }), 0.75);
    const seeked = await video.evaluate((element: HTMLVideoElement) => element.currentTime);
    assert.ok(Math.abs(seeked - 0.75 * duration) < 0.35, `seeked to ${seeked} of ${duration}`);
    await page.keyboard.press("ArrowRight");
    const stepped = await video.evaluate((element: HTMLVideoElement) => element.currentTime);
    assert.ok(stepped > seeked && stepped - seeked < 0.15, `one frame from ${seeked} is ${stepped}`);
  });

  await t.test("frame at playhead and a contact sheet", async () => {
    await action("Frame at playhead").click();
    await until(page, async () => /Frame at \d:\d\d\.\d\d/.test(await stageMeta(page)), "a frame on the stage", STEP_MS);
    assert.equal(await page.getByRole("tab", { name: "Still" }).getAttribute("aria-selected"), "true");

    await action("Contact sheet").click();
    await page.locator(".sheet img").waitFor({ timeout: STEP_MS });
    const chips = page.locator(".sheet .moment button");
    assert.ok((await chips.count()) >= 2, "the sheet lists its frames");
    await chips.nth(1).click();
    assert.equal(await page.getByRole("tab", { name: "Render" }).getAttribute("aria-selected"), "true");
  });

  await t.test("QA findings jump to their line", async () => {
    await action("QA").click();
    // QA may report the overflow differently per frame; any one of them jumps.
    const finding = page.locator(".finding", { hasText: "safe_area" }).first();
    await finding.waitFor({ timeout: STEP_MS });
    assert.equal(await page.locator("#inspector-tab-findings").getAttribute("aria-selected"), "true");
    const link = finding.locator("button.link");
    const line = Number((await link.innerText()).split(":").at(-1));
    await link.click();
    await until(page, async () => (await activeLine(page)) === line && (await editorFocused(page)), `editor focused at line ${line}`);
  });

  await t.test("a save that lost a race offers to reload", async () => {
    await page.locator(".cm-line", { hasText: "self.place(growth_chart(self))" }).click();
    await page.keyboard.press("End");
    await page.keyboard.type("  # mine");
    appendFileSync(join(engine.root, "scenes.py"), "# from another editor\n");
    await page.keyboard.press("Control+s");
    const banner = page.locator(".code .banner", { hasText: "changed on disk" });
    await banner.waitFor({ timeout: STEP_MS });
    await banner.getByRole("button", { name: "Reload from disk" }).click();
    await banner.waitFor({ state: "detached", timeout: STEP_MS });
    const source = readFileSync(join(engine.root, "scenes.py"), "utf8");
    assert.ok(source.endsWith("# from another editor\n") && !source.includes("# mine"));
    assert.equal(await page.evaluate(() => document.querySelector(".cm-content")?.textContent?.includes("# mine")), false);
  });

  await t.test("unsaved edits wait out a lost connection, guarded against leaving", async () => {
    await page.locator(".cm-line", { hasText: "self.place(growth_chart(self))" }).click();
    await page.keyboard.press("End");
    await page.keyboard.type("  # offline");
    assert.equal(await leaveIsGuarded(page), true);

    outage = true;
    await page.route("**/api/**", (route) => route.abort());
    await page.keyboard.press("Control+s");
    await page.getByRole("heading", { name: "Not connected to the engine" }).waitFor();
    assert.equal(await page.locator(".workbench").isVisible(), false);
    // The command names the project's path, which may be long: it scrolls in its box, and Copy stays in view.
    for (const width of [390, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      const fits = await page.evaluate(() => {
        const copy = document.querySelector(".command button")!.getBoundingClientRect();
        return document.documentElement.scrollWidth <= innerWidth && copy.right <= innerWidth;
      });
      assert.equal(fits, true, `the not-connected screen fits ${width} px`);
    }
    await page.unroute("**/api/**");
    await page.locator(".workbench").waitFor({ timeout: STEP_MS });
    outage = false;

    await page.keyboard.press("Control+s");
    await until(page, async () => (await page.locator(".code > .bar").innerText()).includes("Saved"), "saved after reconnecting");
    assert.match(readFileSync(join(engine.root, "scenes.py"), "utf8"), /growth_chart\(self\)\)  # offline\n/);
    assert.equal(await leaveIsGuarded(page), false);
  });

  await t.test("a running render cancels from the stage", async () => {
    await sceneButton("GeneralizedFibonacci").click();
    await action("Preview").click();
    const activity = page.locator(".activity");
    await until(page, async () => (await activity.innerText().catch(() => "")).includes("animate"), "Manim animating", STEP_MS);
    await activity.getByRole("button", { name: "Cancel" }).click();
    await activity.waitFor({ state: "detached", timeout: STEP_MS });
    const newest = (await engine.state()).jobs[0];
    assert.equal(newest?.status, "cancelled");
  });

  await t.test("an export downloads from the job tray", async () => {
    await sceneButton("SequenceData").click();
    await actions.getByRole("button", { name: "Export" }).click();
    await page.getByRole("menuitem", { name: "WebM video" }).click();
    const link = page.locator("#job-tray").getByRole("link", { name: "Download" }).first();
    await link.waitFor({ timeout: STEP_MS });
    const [download] = await Promise.all([page.waitForEvent("download"), link.click()]);
    assert.equal(download.suggestedFilename(), "SequenceData.webm");
  });

  assert.deepEqual(problems, [], `browser errors\n${engine.stderrTail}`);
});

async function until(page: Page, check: () => Promise<boolean>, what: string, timeoutMs = 10_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!(await check())) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for: ${what}`);
    await page.waitForTimeout(100);
  }
}

/** Whether leaving the page now would make the browser ask first. */
function leaveIsGuarded(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    return event.defaultPrevented;
  });
}

async function activeLine(page: Page): Promise<number> {
  return Number(await page.locator(".cm-activeLineGutter").innerText().catch(() => "0"));
}

function editorFocused(page: Page): Promise<boolean> {
  return page.evaluate(() => document.activeElement?.closest(".cm-editor") !== null);
}

function stageMeta(page: Page): Promise<string> {
  return page.locator(".stage-meta").innerText().catch(() => "");
}

async function clickAt(target: Locator, fraction: number): Promise<void> {
  const box = await target.boundingBox();
  assert.ok(box, "the target is visible");
  await target.page().mouse.click(box.x + box.width * fraction, box.y + box.height / 2);
}
