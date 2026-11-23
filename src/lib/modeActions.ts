import { useAppStore } from "@/store/useAppStore";
import {
  resolveLaunchSettings,
  toScrollConfig,
  type CameraModeSettings,
  type DisplayMode,
  type MirrorMode,
  type ModeSettings,
} from "./modeSettings";
import { saveModeDefaults, saveModeDefaultsNotice } from "./persist";
import { setScrollFeel } from "./tauri";
import type { CoreSettings } from "./types";

/** Edit one mode's defaults and persist. Writes only the given mode (SPEC-28 §5.1). */
export function editMode<M extends MirrorMode>(
  mode: M,
  patch: Partial<M extends "camera" ? CameraModeSettings : ModeSettings>,
): void {
  useAppStore.getState().updateMode(mode, patch);
  const state = useAppStore.getState();
  void saveModeDefaults(state.modeDefaults).catch(() => {});

  if (mode !== "camera" && "scroll" in patch && patch.scroll) {
    const scroll = (state.modeDefaults[mode] as ModeSettings).scroll;
    const cfg = toScrollConfig(scroll);
    for (const s of state.sessions) {
      if (s.mode === "camera") continue;
      const sessionMode = state.sessionModes[s.id] ?? launchMode(state.currentMode);
      if (sessionMode === mode) {
        void setScrollFeel(s.serial, cfg).catch(() => {});
        state.setDeviceToggle(s.serial, "swipeScroll", scroll.style === "paged");
      }
    }
  }
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
