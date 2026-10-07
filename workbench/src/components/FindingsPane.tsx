import { useMemo, useState } from "react";
import type { DoctorSnapshot, Scene, SceneId } from "../api/types.ts";
import { bySeverity, cleanQa, codeTarget, type CodeTarget, type FindingCard, type ShownFinding } from "../model/findings.ts";
import { formatTime } from "../model/format.ts";
import { sceneName } from "../model/jobs.ts";
import { useWorkbench } from "../store/useWorkbench.ts";
import { Icon } from "./Icon.tsx";

interface FindingsPaneProps {
  /** The workspace's findings and the newest diagnosis's. */
  findings: readonly ShownFinding[];
  /** Whose findings these include, e.g. `Render Recurrence · draft`. */
  diagnosed: string | null;
  doctor: DoctorSnapshot | null;
  selected: Scene | null;
  onJump: (target: CodeTarget) => void;
  onSeek: (sceneId: SceneId, seconds: number) => void;
  onDoctor: () => void;
  doctorBusy: boolean;
}

const GROUP_TITLES = { error: "Errors", warning: "Warnings", info: "Notes" } as const;
const TONES = { error: "danger", warning: "warning", info: undefined } as const;
const SOURCE_LABELS: Record<FindingCard["finding"]["source"], string> = {
  spec: "director.yaml",
  index: "Scene index",
  render: "Render",
  qa: "QA",
  doctor: "Doctor",
  diagnose: "Diagnosis",
};

/** QA, render, doctor and spec findings plus the newest diagnosis, errors first. */
export function FindingsPane({ findings, diagnosed, doctor, selected, onJump, onSeek, onDoctor, doctorBusy }: FindingsPaneProps) {
  const jobs = useWorkbench((state) => state.jobs);
  const latest = useWorkbench((state) => (selected ? state.workspace?.latest[selected.id] : null) ?? null);
  const [onlySelected, setOnlySelected] = useState(false);
  const groups = useMemo(
    () => bySeverity(onlySelected && selected ? findings.filter((finding) => finding.scene_id === selected.id) : findings),
    [findings, onlySelected, selected],
  );
  const passed = selected ? cleanQa(jobs, findings, selected.id, latest) : null;
  const checkedFrames = passed?.job.request.operation === "qa" ? passed.job.request.frames : undefined;

  return (
    <div className="findings">
      <div className="bar">
        <p className="meta">{environment(doctor)}</p>
        <button type="button" className="small" aria-busy={doctorBusy || undefined} disabled={doctorBusy} onClick={onDoctor}>
          {doctorBusy ? "Checking…" : "Re-check environment"}
        </button>
      </div>
      {selected || diagnosed ? (
        <div className="findings-filters">
          {selected ? (
            <label>
              <input type="checkbox" checked={onlySelected} onChange={(event) => setOnlySelected(event.target.checked)} />
              Only {selected.class_name}
            </label>
          ) : null}
          {diagnosed ? <p className="meta">Includes the diagnosis of {diagnosed}.</p> : null}
        </div>
      ) : null}
      {passed && selected ? (
        <p className="card verdict" data-tone={passed.outdated ? undefined : "success"}>
          <Icon name="check" />
          <span>
            QA found no issues in {selected.class_name}
            {checkedFrames ? ` (${checkedFrames} frames checked)` : ""}.
          </span>
          {passed.outdated ? (
            <span className="tag" data-tone="warning" title="It checked an earlier render, or the scene changed since.">
              Outdated
            </span>
          ) : null}
        </p>
      ) : null}
      {groups.length === 0 && !passed ? (
        <div className="pane-empty">
          <p>No findings.</p>
          <p className="meta">QA checks a render's frames for blank frames, low contrast and content outside the safe area.</p>
        </div>
      ) : null}
      {groups.map((group) => (
        <section key={group.severity} aria-labelledby={`findings-${group.severity}`}>
          <h3 id={`findings-${group.severity}`} className="group-title">
            {GROUP_TITLES[group.severity]} <span className="muted">{group.cards.length}</span>
          </h3>
          <ul>
            {group.cards.map((card) => (
              <FindingItem key={card.key} card={card} onJump={onJump} onSeek={onSeek} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

interface FindingItemProps {
  card: FindingCard;
  onJump: (target: CodeTarget) => void;
  onSeek: (sceneId: SceneId, seconds: number) => void;
}

function FindingItem({ card, onJump, onSeek }: FindingItemProps) {
  const { finding, moments } = card;
  const target = codeTarget(finding.location);
  const { location, scene_id: sceneId } = finding;
  return (
    <li className="card finding" data-tone={TONES[finding.severity]}>
      <p>{finding.message}</p>
      {finding.hint ? <p className="muted">{finding.hint}</p> : null}
      <p className="row meta finding-meta">
        <span>
          {SOURCE_LABELS[finding.source]} · {finding.code}
        </span>
        {sceneId ? <span>{sceneName(sceneId)}</span> : null}
        {target ? (
          <button type="button" className="link mono" onClick={() => onJump(target)}>
            {target.path}:{target.line}
          </button>
        ) : location ? (
          <span className="mono">
            {location.file}:{location.line}
          </span>
        ) : null}
        {finding.outdated ? <span className="warning">Outdated</span> : null}
      </p>
      {moments.length > 0 ? (
        <ul className="row moments" aria-label="When it happens">
          {moments.map(({ at_seconds: at, frame_url: frame }) => (
            <li key={at} className="moment">
              <button
                type="button"
                className="mono"
                disabled={!sceneId}
                aria-label={`Show the render at ${formatTime(at)}`}
                onClick={() => sceneId && onSeek(sceneId, at)}
              >
                {formatTime(at)}
              </button>
              {frame ? (
                <a href={frame} target="_blank" rel="noreferrer" title="Open the measured frame" aria-label={`Measured frame at ${formatTime(at)}`}>
                  <Icon name="image" />
                </a>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
    </li>
  );
}

function environment(doctor: DoctorSnapshot | null): string {
  if (!doctor) return "Environment not checked yet.";
  const { report } = doctor;
  const manim = report.checks.find((check) => check.name === "manim")?.version;
  const missing = [
    !report.capabilities.render && "rendering",
    !report.capabilities.latex && "LaTeX",
    !report.capabilities.video_tools && "video tools",
  ].filter(Boolean);
  const parts = [`Python ${report.runtime.python}`, manim ? `Manim ${manim}` : "Manim missing"];
  if (missing.length > 0) parts.push(`missing ${missing.join(", ")}`);
  return parts.join(" · ");
}
