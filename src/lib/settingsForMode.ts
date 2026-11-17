import type { CoreSettings } from "@/lib/types";

export type SettingsMode = "flex" | "mirror" | "camera" | "hifi";

/**
 * Persisted settings after switching studio mode. Never touches `noAudio`:
 * Camera's audio choice is per-launch (CameraLaunchDialog), not global.
 */
export function settingsForMode(base: CoreSettings, mode: SettingsMode): CoreSettings {
  switch (mode) {
    case "flex":
      return {
        ...base,
        flexDisplay: true,
        embedded: true,
        noWindowAspectRatioLock: true,
        renderFit: "unscaled",
      };
    case "mirror":
      return {
        ...base,
        flexDisplay: false,
        embedded: true,
        noWindowAspectRatioLock: false,
        renderFit: "letterbox",
      };
    case "camera":
      return { ...base, flexDisplay: false, embedded: true, stayAwake: true };
    case "hifi":
      return {
        ...base,
        embedded: true,
        maxFps: 60,
        videoBitRate: 20_000_000,
        videoCodec: "av1",
      };
  }
}
