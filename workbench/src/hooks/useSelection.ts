import { useCallback, useState } from "react";
import type { Profile, Scene, SceneId } from "../api/types.ts";
import { PREVIEW_PROFILE } from "../model/actions.ts";
import { readPreference, writePreference } from "../preferences.ts";

export interface Selection {
  scene: Scene | null;
  selectScene: (id: SceneId) => void;
  profile: string;
  selectProfile: (name: string) => void;
}

/**
 * The selected scene and render profile, remembered per project. Falls back
 * to the first scene and the engine's default profile when the remembered
 * one is gone.
 */
export function useSelection(root: string, scenes: readonly Scene[], profiles: readonly Profile[]): Selection {
  const [sceneChoice, setSceneChoice] = useState(() => readPreference(`scene:${root}`));
  const [profileChoice, setProfileChoice] = useState(() => readPreference(`profile:${root}`));

  const selectScene = useCallback(
    (id: SceneId) => {
      setSceneChoice(id);
      writePreference(`scene:${root}`, id);
    },
    [root],
  );
  const selectProfile = useCallback(
    (name: string) => {
      setProfileChoice(name);
      writePreference(`profile:${root}`, name);
    },
    [root],
  );

  const scene = scenes.find((candidate) => candidate.id === sceneChoice) ?? scenes[0] ?? null;
  const profile = profiles.find((candidate) => candidate.name === profileChoice)
    ?? profiles.find((candidate) => candidate.is_default)
    ?? profiles[0];
  return { scene, selectScene, profile: profile?.name ?? PREVIEW_PROFILE, selectProfile };
}
