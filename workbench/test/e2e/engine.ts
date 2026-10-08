import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";
import type { WorkspaceState } from "../../src/api/types.ts";

const REPO = resolve(import.meta.dirname, "../../..");
const WORKBENCH_DIST = join(REPO, "workbench/dist");
const EXAMPLE = join(REPO, "examples/generalized-fibonacci");
const STARTUP_MS = 30_000;
const STDERR_TAIL = 40;

/** `MANIM_DIRECTOR_BIN` when set, else a cargo build of this checkout; `null` when it does not exist. */
export function engineBinary(): string | null {
  const explicit = process.env.MANIM_DIRECTOR_BIN;
  const candidates = explicit
    ? [explicit]
    : [join(REPO, "target/debug/manim-director"), join(REPO, "target/release/manim-director")];
  return candidates.find((path) => existsSync(path)) ?? null;
}

/** A running `manim-director serve` on a scratch copy of the flagship example. */
export class Engine {
  readonly root: string;
  readonly url: string;
  readonly origin: string;
  readonly #token: string;
  readonly #child: ChildProcess;
  readonly #stderr: string[];

  private constructor(root: string, url: string, child: ChildProcess, stderr: string[]) {
    this.root = root;
    this.url = url;
    const parsed = new URL(url);
    this.origin = parsed.origin;
    this.#token = parsed.searchParams.get("token") ?? "";
    this.#child = child;
    this.#stderr = stderr;
  }

  static async start(binary: string): Promise<Engine> {
    const root = mkdtempSync(join(tmpdir(), "manim-director-e2e-"));
    cpSync(EXAMPLE, root, { recursive: true });
    // Serve this checkout's bundle, which `npm run test:e2e` has just built.
    const child = spawn(binary, ["--project", root, "--json", "serve", "--port", "0", "--workbench-dir", WORKBENCH_DIST], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stderr: string[] = [];
    createInterface({ input: child.stderr! }).on("line", (line) => {
      stderr.push(line);
      if (stderr.length > STDERR_TAIL) stderr.shift();
    });
    try {
      const url = await listeningUrl(child, stderr);
      return new Engine(root, url, child, stderr);
    } catch (error) {
      child.kill("SIGKILL");
      rmSync(root, { recursive: true, force: true });
      throw error;
    }
  }

  /** The engine's last stderr lines, for failure messages. */
  get stderrTail(): string {
    return this.#stderr.join("\n");
  }

  async api<T>(path: string, init: { method?: string; body?: unknown } = {}): Promise<T> {
    const response = await fetch(`${this.origin}${path}`, {
      method: init.method ?? "GET",
      headers: { authorization: `Bearer ${this.#token}`, "content-type": "application/json", origin: this.origin },
      body: init.body === undefined ? undefined : JSON.stringify(init.body),
    });
    if (!response.ok) throw new Error(`${init.method ?? "GET"} ${path}: HTTP ${response.status} ${await response.text()}`);
    return (await response.json()) as T;
  }

  state(): Promise<WorkspaceState> {
    return this.api<WorkspaceState>("/api/state");
  }

  /** Why this machine cannot render video, in the engine's words; `null` once the startup check says it can. */
  async cannotRender(timeoutMs: number): Promise<string | null> {
    const deadline = Date.now() + timeoutMs;
    let state = await this.state();
    while (!state.doctor && Date.now() < deadline) {
      await new Promise((done) => setTimeout(done, 500));
      state = await this.state();
    }
    const { doctor, scene_index: index } = state;
    if (doctor?.report) {
      const { capabilities, findings } = doctor.report;
      if (capabilities.render && capabilities.video_tools) return null;
      const problems = findings.filter((finding) => finding.severity !== "info").map(said);
      return [`capabilities ${JSON.stringify(capabilities)}`, ...problems].join("\n");
    }
    const reasons = [doctor ? `the environment check failed: ${said(doctor.error)}` : `no environment check finished within ${timeoutMs} ms`];
    if (index.error) reasons.push(`the scene index failed: ${said(index.error)}`);
    if (!doctor && this.stderrTail) reasons.push(this.stderrTail);
    return reasons.join("\n");
  }

  async stop(): Promise<void> {
    if (this.#child.exitCode === null) {
      const exited = new Promise((done) => this.#child.once("exit", done));
      this.#child.kill("SIGTERM");
      const timer = setTimeout(() => this.#child.kill("SIGKILL"), 5000);
      await exited;
      clearTimeout(timer);
    }
    rmSync(this.root, { recursive: true, force: true });
  }
}

/** `code: message`, for an error or a finding. */
function said({ code, message }: { code: string; message: string }): string {
  return `${code}: ${message}`;
}

/** The URL from the `--json` listening line, or the reason the engine did not start. */
function listeningUrl(child: ChildProcess, stderr: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    const fail = (why: string) => reject(new Error(`${why}\n${stderr.join("\n")}`));
    const timer = setTimeout(() => fail(`the engine did not listen within ${STARTUP_MS} ms`), STARTUP_MS);
    child.once("exit", (code) => fail(`the engine exited with ${code}`));
    createInterface({ input: child.stdout! }).once("line", (line) => {
      clearTimeout(timer);
      const event = JSON.parse(line) as { event?: string; url?: string };
      if (event.event === "listening" && event.url) resolve(event.url);
      else fail(`unexpected first line: ${line}`);
    });
  });
}
