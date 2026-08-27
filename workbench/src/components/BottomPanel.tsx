import { useState } from "react";
import { ChevronDown, ChevronUp, Download, Pin, RotateCcw, X } from "lucide-react";
import type { BottomTab, ExportArtifact, ExportFormat, LogEntry, RenderJob } from "../types";
import { IconButton } from "./Chrome";

interface BottomPanelProps {
  tab: BottomTab;
  logs: LogEntry[];
  jobs: RenderJob[];
  exports: ExportArtifact[];
  onTabChange: (tab: BottomTab) => void;
  onClearLogs: () => void;
  onCancelRender: (id: string) => void;
  onRetryRender: (id: string) => void;
  onExport: (format: ExportFormat) => void;
  mediaExportAvailable: boolean;
  captionExportAvailable: boolean;
}

export function BottomPanel({ tab, logs, jobs, exports, onTabChange, onClearLogs, onCancelRender, onRetryRender, onExport, mediaExportAvailable, captionExportAvailable }: BottomPanelProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [pinned, setPinned] = useState(false);
  const activeJobs = jobs.filter((job) => job.status === "queued" || job.status === "rendering");
  return (
    <section className={`bottom-panel ${collapsed ? "collapsed" : ""}`} aria-label="Output panel">
      <div className="bottom-tabs">
        <div>
          <button type="button" className={tab === "console" ? "active" : ""} onClick={() => { onTabChange("console"); setCollapsed(false); }}>Console</button>
          <button type="button" className={tab === "queue" ? "active" : ""} onClick={() => { onTabChange("queue"); setCollapsed(false); }}>Render Queue <span>{activeJobs.length}</span></button>
          <button type="button" className={tab === "exports" ? "active" : ""} onClick={() => { onTabChange("exports"); setCollapsed(false); }}>Exports</button>
        </div>
        <div className="panel-actions">
          {tab === "console" ? <button type="button" onClick={onClearLogs}>Clear</button> : null}
          <IconButton label={pinned ? "Unpin output panel" : "Pin output panel"} active={pinned} onClick={() => setPinned((value) => !value)}><Pin size={14} /></IconButton>
          <IconButton label={collapsed ? "Expand output panel" : "Collapse output panel"} onClick={() => setCollapsed((value) => !value)}>{collapsed ? <ChevronUp size={15} /> : <ChevronDown size={15} />}</IconButton>
        </div>
      </div>
      {collapsed ? null : (
        <div className="bottom-content">
          {tab === "console" ? (
            <div className="console-output" role="log" aria-live="polite">
              {logs.length ? logs.map((entry) => (
                <div className={`log-row ${entry.level.toLowerCase()}`} key={entry.id}>
                  <time>[{entry.time}]</time><strong>{entry.level}</strong><span>{entry.message}</span><code>{entry.source}</code>
                </div>
              )) : <div className="empty-output">Console cleared. New engine events will appear here.</div>}
            </div>
          ) : null}

          {tab === "queue" ? (
            <div className="queue-list">
              {jobs.length ? jobs.map((job) => (
                <article className="queue-row" key={job.id}>
                  <div className={`job-state ${job.status}`} />
                  <div className="job-name"><strong>{job.scene}</strong><span>{job.profile} · {job.renderer}</span></div>
                  <div className="job-progress"><div><i style={{ width: `${job.progress}%` }} /></div><span>{job.progress}% · {job.frames}/{job.totalFrames}</span></div>
                  <span className={`status-label ${job.status}`}>{job.status}</span>
                  {job.status === "rendering" || job.status === "queued" ? <button type="button" className="job-action" onClick={() => onCancelRender(job.id)}><X size={13} />Cancel</button> : null}
                  {job.status === "failed" || job.status === "cancelled" ? <button type="button" className="job-action" onClick={() => onRetryRender(job.id)}><RotateCcw size={13} />Retry</button> : null}
                </article>
              )) : <div className="empty-output">No renders queued.</div>}
            </div>
          ) : null}

          {tab === "exports" ? (
            <div className="exports-content">
              <div className="export-presets" aria-label="Export presets">
                <button type="button" onClick={() => onExport("bundle")}>Project bundle</button>
                <button type="button" disabled={!mediaExportAvailable} title={mediaExportAvailable ? "Export MP4" : "Render a video first"} onClick={() => onExport("mp4")}>MP4</button>
                <button type="button" disabled={!mediaExportAvailable} title={mediaExportAvailable ? "Export WebM" : "Render a video first"} onClick={() => onExport("webm")}>WebM</button>
                <button type="button" disabled={!mediaExportAvailable} title={mediaExportAvailable ? "Export GIF" : "Render a video first"} onClick={() => onExport("gif")}>GIF</button>
                <button type="button" disabled={!captionExportAvailable} title={captionExportAvailable ? "Export caption sidecars" : "Add a VTT or SRT source first"} onClick={() => onExport("captions")}>Captions</button>
              </div>
              <div className="artifact-list">
                {exports.map((artifact) => (
                  <article key={artifact.id}>
                    <span className="format-box">{artifact.format}</span>
                    <div><strong>{artifact.name}</strong><small>{artifact.size} · {artifact.createdAt}</small></div>
                    {artifact.url ? <a href={artifact.url} download={artifact.name} aria-label={`Download ${artifact.name}`}><Download size={15} /></a> : <IconButton label="Create a fresh project bundle" onClick={() => onExport("bundle")}><Download size={15} /></IconButton>}
                  </article>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}

interface StatusBarProps {
  projectName: string;
  sceneCount: number;
  duration: number;
  fps: number;
  message: string;
  activeJob?: RenderJob;
  onCancel: (id: string) => void;
}

export function StatusBar({ projectName, sceneCount, duration, fps, message, activeJob, onCancel }: StatusBarProps) {
  return (
    <footer className="statusbar">
      <div><span>Project: <strong>{projectName}</strong></span><span>Scenes: <strong>{sceneCount}</strong></span><span>Duration: <strong>00:00:{duration.toFixed(2).padStart(5, "0")}</strong></span><span>FPS: <strong>{fps}</strong></span></div>
      <div className="engine-message" title={message}>{message}</div>
      {activeJob ? (
        <div className="active-render">
          <span>Render: {activeJob.scene} ({activeJob.frames}/{activeJob.totalFrames} frames)</span>
          <div><i style={{ width: `${activeJob.progress}%` }} /></div>
          <strong>{activeJob.progress}%</strong>
          <button type="button" onClick={() => onCancel(activeJob.id)}>Cancel</button>
        </div>
      ) : <div className="ready-state">Ready</div>}
    </footer>
  );
}
