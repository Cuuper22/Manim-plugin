import {
  Box,
  Camera,
  Code2,
  FileCode2,
  FileText,
  Film,
  Folder,
  FolderOpen,
  Image,
  ListTree,
  Menu,
  Music2,
  PanelLeft,
  PanelRight,
  Play,
  Save,
  Settings,
  Share2,
  Type,
} from "lucide-react";
import { useMemo, useState } from "react";
import type { AssetItem, ConnectionMode, LeftTab, QualityProfile, Renderer, SceneItem } from "../types";

interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  active?: boolean;
}

export function IconButton({ label, active = false, className = "", children, ...props }: IconButtonProps) {
  return (
    <button
      type="button"
      className={`icon-button ${active ? "is-active" : ""} ${className}`}
      aria-label={label}
      title={label}
      {...props}
    >
      {children}
    </button>
  );
}

interface TopBarProps {
  projectName: string;
  profile: QualityProfile;
  renderer: Renderer;
  connection: ConnectionMode;
  rendering: boolean;
  canRender: boolean;
  canSave: boolean;
  onProfileChange: (profile: QualityProfile) => void;
  onRendererChange: (renderer: Renderer) => void;
  onRender: () => void;
  onExport: () => void;
  onToggleProject: () => void;
  onToggleInspector: () => void;
  onSave: () => void;
  onSnapshot: () => void;
}

const profiles: QualityProfile[] = ["Draft 480p", "Preview 720p", "Production 1080p", "Cinema 4K"];
const renderers: Renderer[] = ["Cairo", "OpenGL"];

export function TopBar({
  projectName,
  profile,
  renderer,
  connection,
  rendering,
  canRender,
  canSave,
  onProfileChange,
  onRendererChange,
  onRender,
  onExport,
  onToggleProject,
  onToggleInspector,
  onSave,
  onSnapshot,
}: TopBarProps) {
  return (
    <header className="topbar">
      <div className="brand-lockup">
        <button type="button" className="wordmark" onClick={onToggleProject} aria-label="Toggle project explorer">
          Manim <strong>Director</strong>
        </button>
        <IconButton label="Main menu"><Menu size={18} /></IconButton>
        <span className={`connection-dot ${connection}`} aria-label={`Engine ${connection}`} title={`Engine ${connection}`} />
      </div>

      <div className="history-tools" aria-label="Project history">
        <IconButton label="Save source" onClick={onSave} disabled={!canSave}><Save size={15} /></IconButton>
        <IconButton label="Export snapshot" onClick={onSnapshot}><FileText size={15} /></IconButton>
      </div>

      <div className="project-title" title={projectName}>{projectName}</div>

      <div className="render-tools">
        <label className="compact-select wide">
          <span>Render Profile</span>
          <select value={profile} onChange={(event) => onProfileChange(event.target.value as QualityProfile)}>
            {profiles.map((item) => <option key={item}>{item}</option>)}
          </select>
        </label>
        <label className="compact-select">
          <span>Renderer</span>
          <select value={renderer} onChange={(event) => onRendererChange(event.target.value as Renderer)}>
            {renderers.map((item) => <option key={item}>{item}</option>)}
          </select>
        </label>
        <button type="button" className="primary-button render-button" onClick={onRender} disabled={rendering || !canRender} title={canRender ? "Render selected scene" : "Declare a scene before rendering"}>
          <Play size={14} fill="currentColor" />
          {rendering ? "Rendering" : "Render"}
        </button>
        <button type="button" className="export-button" onClick={onExport}>
          <Share2 size={15} />
          <span>Export</span>
        </button>
        <IconButton label="Toggle inspector" onClick={onToggleInspector}><PanelRight size={17} /></IconButton>
        <IconButton label="Settings"><Settings size={17} /></IconButton>
      </div>
    </header>
  );
}

interface ActivityRailProps {
  active: string;
  onAction: (action: string) => void;
}

export function ActivityRail({ active, onAction }: ActivityRailProps) {
  const tools = [
    ["project", FolderOpen, "Project explorer"],
    ["code", Code2, "Source code"],
    ["camera", Camera, "Camera"],
    ["audio", Music2, "Audio"],
    ["text", Type, "Text"],
    ["objects", Box, "Objects"],
    ["outline", ListTree, "Scene outline"],
  ] as const;
  return (
    <nav className="activity-rail" aria-label="Workbench tools">
      <div>
        {tools.map(([id, Icon, label]) => (
          <IconButton key={id} label={label} active={active === id} onClick={() => onAction(id)}>
            <Icon size={18} />
          </IconButton>
        ))}
      </div>
      <IconButton label="Settings" onClick={() => onAction("settings")}><Settings size={18} /></IconButton>
    </nav>
  );
}

interface ProjectExplorerProps {
  open: boolean;
  projectName: string;
  tab: LeftTab;
  scenes: SceneItem[];
  assets: AssetItem[];
  selectedSceneId: string;
  onTabChange: (tab: LeftTab) => void;
  onSceneSelect: (scene: SceneItem) => void;
  onClose: () => void;
}

function formatRange(start: number, end: number) {
  return `${Math.floor(start / 60)}:${String(Math.floor(start % 60)).padStart(2, "0")}–${Math.floor(end / 60)}:${String(Math.floor(end % 60)).padStart(2, "0")}`;
}

export function ProjectExplorer({ open, projectName, tab, scenes, assets, selectedSceneId, onTabChange, onSceneSelect, onClose }: ProjectExplorerProps) {
  const [query, setQuery] = useState("");
  const normalizedQuery = query.trim().toLowerCase();
  const visibleScenes = useMemo(() => normalizedQuery ? scenes.filter((scene) => `${scene.name} ${scene.file}`.toLowerCase().includes(normalizedQuery)) : scenes, [normalizedQuery, scenes]);
  const visibleAssets = useMemo(() => normalizedQuery ? assets.filter((asset) => `${asset.name} ${asset.type} ${asset.path}`.toLowerCase().includes(normalizedQuery)) : assets, [assets, normalizedQuery]);
  const sourceFiles = useMemo(() => Array.from(new Set(scenes.map((scene) => scene.file).filter(Boolean))), [scenes]);
  return (
    <aside className={`project-explorer ${open ? "is-open" : ""}`} aria-label="Project explorer">
      <div className="panel-tabs">
        <button type="button" className={tab === "project" ? "active" : ""} onClick={() => onTabChange("project")}>Project</button>
        <button type="button" className={tab === "assets" ? "active" : ""} onClick={() => onTabChange("assets")}>Assets</button>
        <IconButton label="Close explorer" className="mobile-close" onClick={onClose}><PanelLeft size={16} /></IconButton>
      </div>
      <div className="explorer-search">
        <input aria-label={`Search ${tab}`} placeholder={tab === "project" ? "Search scenes…" : "Search assets…"} value={query} onChange={(event) => setQuery(event.target.value)} />
      </div>
      {tab === "project" ? (
        <div className="tree" role="tree" aria-label="Project files">
          <div className="tree-root"><FileText size={14} />{projectName}</div>
          <div className="tree-group">
            <div className="tree-folder"><Folder size={14} />Scenes</div>
            <div className="tree-children">
              {visibleScenes.map((scene) => {
                const index = scenes.findIndex((item) => item.id === scene.id);
                return (
                <button
                  type="button"
                  role="treeitem"
                  aria-selected={selectedSceneId === scene.id}
                  className={`scene-row ${selectedSceneId === scene.id ? "selected" : ""}`}
                  key={scene.id}
                  onClick={() => onSceneSelect(scene)}
                >
                  <span className="scene-index">{index + 1}</span>
                  <span>{scene.name}</span>
                  <time>{formatRange(scene.start, scene.end)}</time>
                </button>
                );
              })}
              {visibleScenes.length === 0 ? <div className="tree-empty">No declared scenes</div> : null}
            </div>
            {sourceFiles.length ? <details open className="tree-details">
              <summary><Folder size={14} />Source files</summary>
              <div className="tree-children file-list">
                {sourceFiles.map((file) => <span key={file} title={file}><FileCode2 size={13} />{file.split(/[\\/]/).at(-1)}</span>)}
              </div>
            </details> : null}
          </div>
        </div>
      ) : (
        <div className="asset-list">
          {visibleAssets.map((asset) => {
            const Icon = asset.type === "audio" ? Music2 : asset.type === "video" ? Film : asset.type === "image" || asset.type === "svg" ? Image : Type;
            return (
              <button type="button" key={asset.id} className="asset-row" title={asset.path}>
                <span className={`asset-icon ${asset.type}`}><Icon size={16} /></span>
                <span><strong>{asset.name}</strong><small>{asset.type.toUpperCase()} · {asset.size}</small></span>
              </button>
            );
          })}
          {visibleAssets.length === 0 ? <div className="asset-empty">No matching assets</div> : null}
        </div>
      )}
    </aside>
  );
}
