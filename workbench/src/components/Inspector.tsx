import { useEffect, useState } from "react";
import { Box, ChevronDown, ExternalLink, Eye, EyeOff, Lock, Save, Unlock, X } from "lucide-react";
import type { InspectorTab, SelectedObject } from "../types";
import { IconButton } from "./Chrome";

interface InspectorProps {
  open: boolean;
  tab: InspectorTab;
  selection: SelectedObject;
  sourceCode: string;
  editableVisuals: boolean;
  sourceReady: boolean;
  onTabChange: (tab: InspectorTab) => void;
  onSelectionChange: (selection: SelectedObject, persist?: boolean) => void;
  onSourceSave: (source: string) => Promise<boolean> | boolean;
  onClose: () => void;
}

function NumberField({ value, label, step = 0.1, disabled = false, onChange, onCommit }: { value: number; label: string; step?: number; disabled?: boolean; onChange: (value: number) => void; onCommit?: () => void }) {
  return (
    <label className="number-field">
      <span>{label}</span>
      <input
        aria-label={label}
        type="number"
        value={Number.isInteger(value) ? value.toFixed(2) : value}
        step={step}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
        onBlur={onCommit}
      />
    </label>
  );
}

function InspectorSection({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <details className="inspector-section" open>
      <summary><ChevronDown size={14} />{title}</summary>
      <div className="section-fields">{children}</div>
    </details>
  );
}

export function Inspector({ open, tab, selection, sourceCode, editableVisuals, sourceReady, onTabChange, onSelectionChange, onSourceSave, onClose }: InspectorProps) {
  const [draftCode, setDraftCode] = useState(sourceCode);
  const [saved, setSaved] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDraftCode(sourceCode);
    setSaved(true);
  }, [sourceCode]);

  const patch = <K extends keyof SelectedObject>(key: K, value: SelectedObject[K], persist = false) => {
    if (!editableVisuals) return;
    onSelectionChange({ ...selection, [key]: value }, persist);
  };

  const tuplePatch = (key: "position" | "scale", index: number, value: number, persist = false) => {
    const tuple = [...selection[key]] as [number, number, number];
    tuple[index] = value;
    patch(key, tuple, persist);
  };

  const commitSelection = () => {
    if (editableVisuals) onSelectionChange(selection, true);
  };

  const saveDraft = async () => {
    if (!sourceReady || saving) return;
    setSaving(true);
    try {
      if (await onSourceSave(draftCode)) setSaved(true);
    } finally {
      setSaving(false);
    }
  };

  return (
    <aside className={`inspector ${open ? "is-open" : ""}`} aria-label="Object inspector">
      <div className="panel-tabs inspector-tabs">
        <button type="button" className={tab === "inspector" ? "active" : ""} onClick={() => onTabChange("inspector")}>Inspector</button>
        <button type="button" className={tab === "code" ? "active" : ""} onClick={() => onTabChange("code")}>Code</button>
        <IconButton label="Close inspector" className="mobile-close" onClick={onClose}><X size={16} /></IconButton>
      </div>

      {tab === "inspector" ? (
        <div className="inspector-content">
          <p className="section-kicker">Selection</p>
          <div className="object-header">
            <Box size={24} />
            <div><strong>{selection.name}</strong><span>{selection.type}</span></div>
            <IconButton label={editableVisuals ? (selection.visible ? "Hide object" : "Show object") : "Visibility is inspect-only while connected"} disabled={!editableVisuals} onClick={() => patch("visible", !selection.visible, true)}>
              {selection.visible ? <Eye size={15} /> : <EyeOff size={15} />}
            </IconButton>
            <IconButton label={editableVisuals ? (selection.locked ? "Unlock object" : "Lock object") : "Lock state is inspect-only while connected"} disabled={!editableVisuals} onClick={() => patch("locked", !selection.locked, true)}>
              {selection.locked ? <Lock size={15} /> : <Unlock size={15} />}
            </IconButton>
          </div>

          {!editableVisuals ? (
            <div className="inspect-only-note" role="note">
              <Lock size={14} aria-hidden="true" />
              <span><strong>Inspect only while connected.</strong> Edit Code or use Codex <code>project_apply</code> to make source-backed changes.</span>
            </div>
          ) : null}

          <InspectorSection title="Transform">
            <div className="field-row labeled-row">
              <span>Position</span>
              <div className="triple-fields">
                {(["x", "y", "z"] as const).map((axis, index) => (
                  <NumberField key={axis} label={axis} value={selection.position[index]} disabled={!editableVisuals} onChange={(value) => tuplePatch("position", index, value)} onCommit={commitSelection} />
                ))}
              </div>
            </div>
            <div className="field-row labeled-row">
              <span>Scale</span>
              <div className="triple-fields">
                {(["x", "y", "z"] as const).map((axis, index) => (
                  <NumberField key={axis} label={axis} value={selection.scale[index]} disabled={!editableVisuals} onChange={(value) => tuplePatch("scale", index, value)} onCommit={commitSelection} />
                ))}
              </div>
            </div>
            <label className="field-row"><span>Rotation</span><input value={`${selection.rotation}°`} disabled={!editableVisuals} onChange={(event) => patch("rotation", Number(event.target.value.replace("°", "")) || 0)} onBlur={commitSelection} /></label>
            <label className="field-row"><span>Anchor</span><select value={selection.anchor} disabled={!editableVisuals} onChange={(event) => patch("anchor", event.target.value, true)}><option>CENTER</option><option>ORIGIN</option><option>TOP</option><option>BOTTOM</option><option>LEFT</option><option>RIGHT</option></select></label>
          </InspectorSection>

          <InspectorSection title="Style">
            <label className="field-row color-field"><span>Color</span><i style={{ background: selection.color }} /><input value={selection.color.toUpperCase()} disabled={!editableVisuals} onChange={(event) => patch("color", event.target.value)} onBlur={commitSelection} /></label>
            <label className="field-row"><span>Fill Opacity</span><input type="number" min="0" max="1" step="0.05" value={selection.fillOpacity.toFixed(2)} disabled={!editableVisuals} onChange={(event) => patch("fillOpacity", Number(event.target.value))} onBlur={commitSelection} /></label>
            <label className="field-row color-field"><span>Stroke</span><i style={{ background: selection.stroke }} /><input value={selection.stroke.toUpperCase()} disabled={!editableVisuals} onChange={(event) => patch("stroke", event.target.value)} onBlur={commitSelection} /></label>
            <label className="field-row"><span>Stroke Width</span><input type="number" min="0" step="0.25" value={selection.strokeWidth.toFixed(1)} disabled={!editableVisuals} onChange={(event) => patch("strokeWidth", Number(event.target.value))} onBlur={commitSelection} /></label>
          </InspectorSection>

          <InspectorSection title="Timing">
            <label className="field-row"><span>Appear</span><input value={`0:${selection.appear.toFixed(2)}`} disabled={!editableVisuals} onChange={(event) => patch("appear", Number(event.target.value.split(":").at(-1)) || 0)} onBlur={commitSelection} /></label>
            <label className="field-row"><span>Start</span><input value={`0:${selection.start.toFixed(2)}`} disabled={!editableVisuals} onChange={(event) => patch("start", Number(event.target.value.split(":").at(-1)) || 0)} onBlur={commitSelection} /></label>
            <label className="field-row"><span>End</span><input value={`0:${selection.end.toFixed(2)}`} disabled={!editableVisuals} onChange={(event) => patch("end", Number(event.target.value.split(":").at(-1)) || 0)} onBlur={commitSelection} /></label>
            <div className="field-row"><span>Fade</span><div className="double-fields"><NumberField label="In" value={selection.fadeIn} disabled={!editableVisuals} onChange={(value) => patch("fadeIn", value)} onCommit={commitSelection} /><NumberField label="Out" value={selection.fadeOut} disabled={!editableVisuals} onChange={(value) => patch("fadeOut", value)} onCommit={commitSelection} /></div></div>
            <label className="field-row"><span>Easing</span><select value={selection.easing} disabled={!editableVisuals} onChange={(event) => patch("easing", event.target.value, true)}><option>smooth</option><option>linear</option><option>ease_in_quad</option><option>ease_out_back</option><option>there_and_back</option></select></label>
          </InspectorSection>

          <InspectorSection title="Source">
            <button type="button" className="source-link" onClick={() => onTabChange("code")}><span>File</span><strong>{selection.source.file}</strong></button>
            <div className="field-row source-line"><span>Line</span><button type="button" onClick={() => onTabChange("code")}>{selection.source.line}</button><button type="button" className="open-source" onClick={() => onTabChange("code")}><ExternalLink size={13} />Open</button></div>
            <div className="field-row"><span>Object</span><code>{selection.source.object}</code></div>
          </InspectorSection>
        </div>
      ) : (
        <div className="code-panel">
          <div className="code-header">
            <span><strong>{selection.source.file ? selection.source.file.split("/").at(-1) : "No source selected"}</strong>{selection.source.file ? <small>line {selection.source.line}</small> : null}</span>
            <button
              type="button"
              className="code-save"
              disabled={saved || saving || !selection.source.file || !sourceReady}
              onClick={() => { void saveDraft(); }}
            ><Save size={14} />{saving ? "Saving…" : !sourceReady && selection.source.file ? "Loading…" : saved ? "Saved" : "Save"}</button>
          </div>
          <div className="code-editor-wrap">
            <div className="line-numbers" aria-hidden="true">
              {draftCode.split("\n").map((_, index) => <span key={index}>{index + 1}</span>)}
            </div>
            <textarea
              aria-label="Manim scene source"
              value={draftCode}
              disabled={!selection.source.file || !sourceReady}
              placeholder={!selection.source.file ? "Select a declared scene to inspect its source." : !sourceReady ? "Loading every source page and locking its revision…" : undefined}
              spellCheck={false}
              onChange={(event) => { setDraftCode(event.target.value); setSaved(false); }}
            />
          </div>
        </div>
      )}
    </aside>
  );
}
