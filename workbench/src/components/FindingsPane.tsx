import { useMemo, useState } from "react";
import type { DoctorSnapshot, Finding, Scene, SceneId } from "../api/types.ts";
import type { Diagnosis } from "../hooks/useDiagnosis.ts";
import { bySeverity, codeTarget, fromWorkspace, type CodeTarget, type ShownFinding } from "../model/findings.ts";
import { formatTime } from "../model/format.ts";
import { sceneName } from "../model/jobs.ts";

interface FindingsPaneProps {
  findings: readonly Finding[];
  diagnosis: Diagnosis | null;
  doctor: DoctorSnapshot | null;
  selected: Scene | null;
  onJump: (target: CodeTarget) => void;
  onSeek: (sceneId: SceneId, seconds: number) => void;
  onDoctor: () => void;
  doctorBusy: boolean;
}

const GROUP_TITLES = { error: "Errors", warning: "Warnings", info: "Notes" } as const;
const TONES = { error: "danger", warning: "warning", info: undefined } as const;
const SOURCE_LABELS: Record<ShownFinding["source"], string> = {
  spec: "director.yaml",
  index: "Scene index",
  render: "Render",
  qa: "QA",
  doctor: "Doctor",
  diagnose: "Diagnosis",
};

/** QA, render, doctor and spec findings plus the newest diagnosis, errors first. */
export function FindingsPane({ findings, diagnosis, doctor, selected, onJump, onSeek, onDoctor, doctorBusy }: FindingsPaneProps) {
  const [onlySelected, setOnlySelected] = useState(false);
  const shown = useMemo(() => {
    const all = [...fromWorkspace(findings), ...(diagnosis?.findings ?? [])];
    return onlySelected && selected ? all.filter((finding) => finding.scene_id === selected.id) : all;
  }, [findings, diagnosis, onlySelected, selected]);
  const groups = bySeverity(shown);

  return (
    <div className="findings">
      <div className="bar">
        <p className="meta">{environment(doctor)}</p>
        <button type="button" className="quiet" disabled={doctorBusy} onClick={onDoctor}>
          {doctorBusy ? "Checking…" : "Re-check environment"}
        </button>
      </div>
      {selected ? (
        <label>
          <input type="checkbox" checked={onlySelected} onChange={(event) => setOnlySelected(event.target.checked)} />
          Only {selected.class_name}
        </label>
      ) : null}
      {diagnosis ? <p className="meta">Includes the diagnosis of {diagnosis.subject}.</p> : null}
      {groups.length === 0 ? <p className="pane-note">No findings.</p> : null}
      {groups.map((group) => (
        <section key={group.severity} aria-labelledby={`findings-${group.severity}`}>
          <h3 id={`findings-${group.severity}`} className="group-title">
            {GROUP_TITLES[group.severity]} <span className="muted">{group.findings.length}</span>
          </h3>
          <ul>
            {group.findings.map((finding) => (
              <FindingItem key={finding.key} finding={finding} onJump={onJump} onSeek={onSeek} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

interface FindingItemProps {
  finding: ShownFinding;
  onJump: (target: CodeTarget) => void;
  onSeek: (sceneId: SceneId, seconds: number) => void;
}

function FindingItem({ finding, onJump, onSeek }: FindingItemProps) {
  const target = codeTarget(finding.location);
  const { location, at_seconds: at, scene_id: sceneId } = finding;
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
          <span>
            {location.file}:{location.line}
          </span>
        ) : null}
        {at !== null && sceneId ? (
          <button type="button" className="link" onClick={() => onSeek(sceneId, at)}>
            at {formatTime(at)}
          </button>
        ) : null}
        {finding.frame_url ? (
          <a href={finding.frame_url} target="_blank" rel="noreferrer">
            Frame
          </a>
        ) : null}
        {finding.outdated ? <span className="warning">Outdated</span> : null}
      </p>
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
