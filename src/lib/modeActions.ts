import { useAppStore } from "@/store/useAppStore";
import {
  resolveLaunchSettings,
  type CameraModeSettings,
  type DisplayMode,
  type MirrorMode,
  type ModeSettings,
} from "./modeSettings";
import { saveModeDefaults, saveModeDefaultsNotice } from "./persist";
import type { CoreSettings } from "./types";

/** Edit one mode's defaults and persist. Writes only the given mode (SPEC-28 §5.1). */
export function editMode<M extends MirrorMode>(
  mode: M,
  patch: Partial<M extends "camera" ? CameraModeSettings : ModeSettings>,
): void {
  useAppStore.getState().updateMode(mode, patch);
  void saveModeDefaults(useAppStore.getState().modeDefaults).catch(() => {});
}

export function dismissModeNotice(): void {
  useAppStore.getState().setModeDefaultsNotice(false);
  void saveModeDefaultsNotice(false).catch(() => {});
}

/** Display mode to launch with: the selected mode, Camera falls back to Mirror's object. */
export function launchMode(mode: MirrorMode): DisplayMode {
  return mode === "camera" ? "mirror" : mode;
}

/** Resolved launch settings for the currently selected mode (Devices, palette, Phone Hub). */
export function resolveSelectedLaunchSettings(): CoreSettings {
  const { currentMode, modeDefaults } = useAppStore.getState();
  return resolveLaunchSettings(launchMode(currentMode), modeDefaults);
}
