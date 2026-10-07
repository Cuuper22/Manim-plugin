import type { ReactNode } from "react";
import type { Profile, Scene, SceneId } from "../api/types.ts";
import type { ThemeChoice } from "../hooks/useTheme.ts";

interface TopBarProps {
  project: string;
  scenes: readonly Scene[];
  scene: Scene | null;
  onScene: (id: SceneId) => void;
  profiles: readonly Profile[];
  profile: string;
  onProfile: (name: string) => void;
  theme: ThemeChoice;
  onTheme: () => void;
  onShortcuts: () => void;
  /** The scene actions. */
  actions: ReactNode;
  /** The job tray. */
  jobs: ReactNode;
}

const THEME_ICONS: Record<ThemeChoice, ReactNode> = {
  system: (
    <>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M8 2.5a5.5 5.5 0 0 0 0 11z" fill="currentColor" />
    </>
  ),
  light: (
    <>
      <circle cx="8" cy="8" r="3" />
      <path d="M8 1v2M8 13v2M1 8h2M13 8h2M3 3l1.4 1.4M11.6 11.6L13 13M3 13l1.4-1.4M11.6 4.4L13 3" />
    </>
  ),
  dark: <path d="M13 9.5A5.5 5.5 0 1 1 6.5 3a4.5 4.5 0 0 0 6.5 6.5z" />,
};

function describe(profile: Profile | undefined): string | undefined {
  if (!profile) return undefined;
  const alpha = profile.transparent ? ", transparent" : "";
  return `${profile.width}×${profile.height}, ${profile.fps} fps, ${profile.format}${alpha} (${profile.renderer})`;
}

export function TopBar(props: TopBarProps) {
  const { scenes, scene, profiles, profile } = props;
  return (
    <header className="bar topbar">
      <h1 className="project" title={props.project}>
        {props.project}
      </h1>
      <div className="row pickers">
        <label>
          <span className="visually-hidden">Scene</span>
          <select value={scene?.id ?? ""} disabled={scenes.length === 0} onChange={(event) => props.onScene(event.target.value)}>
            {scenes.length === 0 ? <option value="">No scenes</option> : null}
            {scenes.map((candidate) => (
              <option key={candidate.id} value={candidate.id}>
                {candidate.class_name}
              </option>
            ))}
          </select>
        </label>
        <label>
          <span className="visually-hidden">Render profile</span>
          <select
            value={profile}
            title={describe(profiles.find((candidate) => candidate.name === profile))}
            onChange={(event) => props.onProfile(event.target.value)}
          >
            {profiles.map((candidate) => (
              <option key={candidate.name} value={candidate.name} title={describe(candidate)}>
                {candidate.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      {props.actions}
      <div className="row utilities">
        {props.jobs}
        <button
          type="button"
          className="icon"
          aria-label={`Color theme: ${props.theme}`}
          title={`Color theme: ${props.theme}`}
          onClick={props.onTheme}
        >
          <svg className="stroke" viewBox="0 0 16 16" aria-hidden="true">
            {THEME_ICONS[props.theme]}
          </svg>
        </button>
        <button type="button" className="icon" aria-label="Keyboard shortcuts" aria-keyshortcuts="?" onClick={props.onShortcuts}>
          ?
        </button>
      </div>
    </header>
  );
}
