// SPEC-28 / ADR-0005: per-mode mirroring defaults, resolver, migration and UI helpers.
// Pure module: no I/O. Persistence lives in settingsStore.ts.
import type {
  AudioBufferSetting,
  CameraSettings,
  CoreSettings,
  PresetVideo,
  QualityBundleId,
} from "./types";
import { DEFAULT_SETTINGS, PRESET_BUNDLES } from "./types";

export type MirrorMode = "mirror" | "flex" | "camera" | "hifi";
/** Modes that launch through `start_mirror` (everything except Camera). */
export type DisplayMode = Exclude<MirrorMode, "camera">;

export const MIRROR_MODES: MirrorMode[] = ["mirror", "flex", "camera", "hifi"];

/** Same fields as CoreSettings minus flexDisplay (derived from the mode), plus scroll settings (SPEC-30). */
export type ScrollStyle = "smooth" | "paged";
export type ScrollSpeed = "auto" | number;
export type ScrollInertia = "off" | "low" | "medium" | "high";

export interface ScrollSettings {
  style: ScrollStyle;
  speed: ScrollSpeed;
  touchpadSpeed: number;
  smooth: boolean;
  inertia: ScrollInertia;
}

export interface ScrollConfigPayload {
  style: ScrollStyle;
  speedMode: "auto" | "manual";
  manual: number;
  touchpadSpeed: number;
  smooth: boolean;
  inertia: ScrollInertia;
}

export const SCROLL_SPEED_OPTIONS: number[] = [
  1, 1.25, 1.5, 1.75, 2, 2.25, 2.5, 2.75, 3, 3.25, 3.5, 3.75, 4, 4.25, 4.5, 4.75, 5,
];

export const TOUCHPAD_SPEED_OPTIONS: number[] = [
  0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 2.25, 2.5, 2.75, 3,
];

export const DEFAULT_SCROLL: Record<MirrorMode, ScrollSettings> = {
  mirror: { style: "smooth", speed: "auto", touchpadSpeed: 1, smooth: true, inertia: "off" },
  flex: { style: "smooth", speed: "auto", touchpadSpeed: 1, smooth: true, inertia: "off" },
  camera: { style: "smooth", speed: "auto", touchpadSpeed: 1, smooth: false, inertia: "off" },
  hifi: { style: "smooth", speed: "auto", touchpadSpeed: 1, smooth: false, inertia: "off" },
};

export function toScrollConfig(s: ScrollSettings): ScrollConfigPayload {
  return {
    style: s.style,
    speedMode: s.speed === "auto" ? "auto" : "manual",
    manual: typeof s.speed === "number" ? s.speed : 1.0,
    touchpadSpeed: s.touchpadSpeed,
    smooth: s.smooth,
    inertia: s.inertia,
  };
}

export type ModeSettings = Omit<CoreSettings, "flexDisplay"> & {
  scroll: ScrollSettings;
};

export type CameraBitrate = "low" | "balanced" | "high";

export interface CameraModeSettings {
  playAudio: boolean; // CameraSettings.noAudio = !playAudio
  bitrate: CameraBitrate; // 2 / 8 / 16 Mbps
  embedded: boolean;
}

export interface ModeDefaultsV2 {
  version: 2;
  mirror: ModeSettings;
  flex: ModeSettings;
  hifi: ModeSettings;
  camera: CameraModeSettings;
}

export const CAMERA_BITRATE_BPS: Record<CameraBitrate, number> = {
  low: 2_000_000,
  balanced: 8_000_000,
  high: 16_000_000,
};

type CoreModeFields = Omit<CoreSettings, "flexDisplay">;

const MODE_SETTINGS_KEYS = (Object.keys(DEFAULT_SETTINGS) as (keyof CoreSettings)[]).filter(
  (k) => k !== "flexDisplay",
) as (keyof CoreModeFields)[];

function stripFlex(s: CoreSettings): CoreModeFields {
  const out: Partial<CoreModeFields> = {};
  for (const k of MODE_SETTINGS_KEYS) {
    (out as Record<string, unknown>)[k] = s[k];
  }
  return out as CoreModeFields;
}

const BASE_MODE_SETTINGS: CoreModeFields = {
  ...stripFlex(DEFAULT_SETTINGS),
  embedded: true,
};

/** Fresh-install defaults (SPEC-28 §5.3 + SPEC-30 §3). Built by a function so callers get fresh objects. */
export function defaultModeDefaults(): ModeDefaultsV2 {
  return {
    version: 2,
    mirror: {
      ...BASE_MODE_SETTINGS,
      ...PRESET_BUNDLES.medium,
      noWindowAspectRatioLock: false,
      renderFit: "letterbox",
      scroll: { ...DEFAULT_SCROLL.mirror },
    },
    flex: {
      ...BASE_MODE_SETTINGS,
      ...PRESET_BUNDLES.medium,
      noWindowAspectRatioLock: true,
      renderFit: "unscaled",
      scroll: { ...DEFAULT_SCROLL.flex },
    },
    hifi: {
      ...BASE_MODE_SETTINGS,
      maxSize: 0,
      videoBitRate: 20_000_000,
      maxFps: 60,
      videoCodec: "h265",
      noWindowAspectRatioLock: false,
      renderFit: "letterbox",
      scroll: { ...DEFAULT_SCROLL.hifi },
    },
    camera: { playAudio: true, bitrate: "balanced", embedded: true },
  };
}

export const DEFAULT_MODE_DEFAULTS: ModeDefaultsV2 = defaultModeDefaults();

/**
 * Launch settings are resolved, never stored. Returns every CoreSettings field;
 * only `flexDisplay` is forced by the mode.
 */
export function resolveLaunchSettings(mode: DisplayMode, defaults: ModeDefaultsV2): CoreSettings {
  const { scroll: _scroll, ...core } = defaults[mode];
  return {
    ...core,
    audioBuffer: normalizeAudioBuffer(core.audioBuffer, "auto"),
    flexDisplay: mode === "flex",
  };
}

/** Camera launch dialog pre-fill: persisted defaults, overridden by per-launch choices. */
export function toCameraSettings(
  camera: CameraModeSettings,
  choices: Pick<CameraSettings, "facing"> & Partial<CameraSettings>,
): CameraSettings {
  return {
    noAudio: !camera.playAudio,
    embedded: camera.embedded,
    videoBitRate: CAMERA_BITRATE_BPS[camera.bitrate],
    ...choices,
  };
}

// --- Normalisation of a persisted object ---------------------------------------------------

export function normalizeAudioBuffer(
  raw: unknown,
  fallback: AudioBufferSetting = "auto",
): AudioBufferSetting {
  return raw === "auto" || raw === 50 || raw === 100 || raw === 200 ? raw : fallback;
}

function normalizeScrollSettings(raw: unknown, fallback: ScrollSettings): ScrollSettings {
  if (!raw || typeof raw !== "object") return { ...fallback };
  const r = raw as Partial<ScrollSettings>;
  const style: ScrollStyle = r.style === "smooth" || r.style === "paged" ? r.style : fallback.style;
  const speed: ScrollSpeed =
    r.speed === "auto"
      ? "auto"
      : typeof r.speed === "number" && Number.isFinite(r.speed)
        ? Math.min(5, Math.max(1, r.speed))
        : fallback.speed;
  const touchpadSpeed =
    typeof r.touchpadSpeed === "number" && Number.isFinite(r.touchpadSpeed)
      ? Math.min(3, Math.max(0.5, r.touchpadSpeed))
      : fallback.touchpadSpeed;
  const smooth = typeof r.smooth === "boolean" ? r.smooth : fallback.smooth;
  const inertia: ScrollInertia =
    r.inertia === "off" || r.inertia === "low" || r.inertia === "medium" || r.inertia === "high"
      ? r.inertia
      : fallback.inertia;
  return { style, speed, touchpadSpeed, smooth, inertia };
}

function pickModeSettings(raw: unknown, fallback: ModeSettings): ModeSettings {
  if (!raw || typeof raw !== "object") return { ...fallback, scroll: { ...fallback.scroll } };
  const rawObj = raw as Partial<ModeSettings>;
  const merged = { ...fallback, ...rawObj };
  return {
    ...stripFlex({ ...merged, flexDisplay: false }),
    audioBuffer: normalizeAudioBuffer(rawObj.audioBuffer, fallback.audioBuffer ?? "auto"),
    scroll: normalizeScrollSettings(rawObj.scroll, fallback.scroll),
  };
}

/** Fill gaps in a persisted `modeDefaults` so a partial or older object still loads. */
export function normalizeModeDefaults(raw: unknown): ModeDefaultsV2 {
  const d = defaultModeDefaults();
  if (!raw || typeof raw !== "object") return d;
  const r = raw as Partial<Record<string, unknown>>;
  const cam = (r.camera && typeof r.camera === "object" ? r.camera : {}) as Partial<CameraModeSettings>;
  return {
    version: 2,
    mirror: pickModeSettings(r.mirror, d.mirror),
    flex: pickModeSettings(r.flex, d.flex),
    hifi: pickModeSettings(r.hifi, d.hifi),
    camera: {
      playAudio: typeof cam.playAudio === "boolean" ? cam.playAudio : d.camera.playAudio,
      bitrate:
        cam.bitrate === "low" || cam.bitrate === "balanced" || cam.bitrate === "high"
          ? cam.bitrate
          : d.camera.bitrate,
      embedded: typeof cam.embedded === "boolean" ? cam.embedded : d.camera.embedded,
    },
  };
}

// --- Migration (SPEC-28 §5.4 steps 1-5) ---------------------------------------------------

export type LegacyPreset = QualityBundleId | "custom";

export interface MigrationResult {
  modeDefaults: ModeDefaultsV2;
  /** True when legacy `noAudio` was on and has been turned on (show the one-time notice). */
  audioNotice: boolean;
}

const HIFI_BITRATE = 20_000_000;
const HIFI_FPS = 60;

export function nearestCameraBitrate(bps: number): CameraBitrate {
  let best: CameraBitrate = "balanced";
  let bestDist = Number.POSITIVE_INFINITY;
  for (const k of ["low", "balanced", "high"] as CameraBitrate[]) {
    const dist = Math.abs(CAMERA_BITRATE_BPS[k] - bps);
    if (dist < bestDist) {
      best = k;
      bestDist = dist;
    }
  }
  return best;
}

/** Pure migration of the legacy global `settings` + `preset` into per-mode defaults. */
export function migrateLegacy(
  legacy: Partial<CoreSettings> | null | undefined,
  preset?: string | null,
): MigrationResult {
  if (!legacy || typeof legacy !== "object") {
    return { modeDefaults: defaultModeDefaults(), audioNotice: false };
  }
  const S: CoreSettings = {
    ...DEFAULT_SETTINGS,
    ...legacy,
    audioBuffer: normalizeAudioBuffer(legacy.audioBuffer, "auto"),
  };
  const bundleId: QualityBundleId =
    preset === "low" || preset === "medium" || preset === "high" || preset === "highest"
      ? preset
      : "medium";

  // 1. Base copy: every mode starts as a copy of S minus flexDisplay.
  const base = stripFlex(S);
  const mirror: ModeSettings = { ...base, scroll: { ...DEFAULT_SCROLL.mirror } };
  const flex: ModeSettings = { ...base, scroll: { ...DEFAULT_SCROLL.flex } };
  const hifi: ModeSettings = { ...base, scroll: { ...DEFAULT_SCROLL.hifi } };

  // 2. Undo preset residue on quality.
  const hifiSignature =
    S.videoCodec === "av1" && S.videoBitRate === HIFI_BITRATE && S.maxFps === HIFI_FPS;
  if (hifiSignature) {
    Object.assign(mirror, PRESET_BUNDLES[bundleId]);
    Object.assign(flex, PRESET_BUNDLES[bundleId]);
  } else {
    hifi.videoBitRate = Math.max(S.videoBitRate, HIFI_BITRATE);
    hifi.maxFps = Math.max(S.maxFps, HIFI_FPS);
  }

  // 3. Undo preset residue on window fit.
  if (S.flexDisplay) {
    mirror.noWindowAspectRatioLock = false;
    mirror.renderFit = "letterbox";
    hifi.noWindowAspectRatioLock = false;
    hifi.renderFit = "letterbox";
  } else {
    flex.noWindowAspectRatioLock = true;
    flex.renderFit = "unscaled";
  }
  if (S.renderFit === "stretched") {
    mirror.renderFit = "stretched";
    flex.renderFit = "stretched";
    hifi.renderFit = "stretched";
  }

  // 4. Audio is never silently forced off.
  mirror.noAudio = false;
  flex.noAudio = false;
  hifi.noAudio = false;

  // 5. Camera.
  const camera: CameraModeSettings = {
    playAudio: true,
    bitrate: nearestCameraBitrate(S.videoBitRate),
    embedded: S.embedded ?? false,
  };

  return {
    modeDefaults: { version: 2, mirror, flex, hifi, camera },
    audioNotice: S.noAudio === true,
  };
}

/**
 * SPEC-28 §5.4 step 6: file copy-paste was effectively off when the retired file gate
 * (`syncFileTransfer`) was off. Returns the KVM settings patch to apply, or null.
 */
export function fileSyncMigrationPatch(kvm: {
  syncFileTransfer?: boolean;
  continuityFileClipboard?: boolean;
}): { continuityFileClipboard: false } | null {
  if (kvm.syncFileTransfer === false && kvm.continuityFileClipboard === true) {
    return { continuityFileClipboard: false };
  }
  return null;
}

// --- Quality and inversion helpers ---------------------------------------------------------

export type QualityChoice = "smooth" | "balanced" | "sharp" | "maximum" | "custom";

export const QUALITY_BUNDLE: Record<Exclude<QualityChoice, "custom">, QualityBundleId> = {
  smooth: "low",
  balanced: "medium",
  sharp: "high",
  maximum: "highest",
};

export const QUALITY_LABELS: Record<QualityChoice, string> = {
  smooth: "Smooth",
  balanced: "Balanced",
  sharp: "Sharp",
  maximum: "Maximum",
  custom: "Custom",
};

export function deriveQuality(s: PresetVideo): QualityChoice {
  for (const choice of Object.keys(QUALITY_BUNDLE) as Exclude<QualityChoice, "custom">[]) {
    const b = PRESET_BUNDLES[QUALITY_BUNDLE[choice]];
    if (
      b.maxSize === s.maxSize &&
      b.videoBitRate === s.videoBitRate &&
      b.maxFps === s.maxFps &&
      b.videoCodec === s.videoCodec
    ) {
      return choice;
    }
  }
  return "custom";
}

/** The four quality fields for a bundle choice; null for Custom. */
export function applyQuality(choice: QualityChoice): PresetVideo | null {
  if (choice === "custom") return null;
  return { ...PRESET_BUNDLES[QUALITY_BUNDLE[choice]] };
}

export type ResolutionSegment = "native" | "1080p" | "720p" | "custom";
export const RESOLUTION_VALUE: Record<Exclude<ResolutionSegment, "custom">, number> = {
  native: 0,
  "1080p": 1920,
  "720p": 1280,
};
export function segmentForResolution(maxSize: number): ResolutionSegment {
  if (maxSize === 0) return "native";
  if (maxSize === 1920) return "1080p";
  if (maxSize === 1280) return "720p";
  return "custom";
}

export type BitrateSegment = "low" | "balanced" | "high";
export const BITRATE_VALUE: Record<BitrateSegment, number> = {
  low: 2_000_000,
  balanced: 8_000_000,
  high: 16_000_000,
};
/** Low <= 4 Mbps, Balanced <= 12 Mbps, High above. */
export function segmentForBitrate(bps: number): BitrateSegment {
  if (bps <= 4_000_000) return "low";
  if (bps <= 12_000_000) return "balanced";
  return "high";
}

export type FpsSegment = 30 | 60 | "custom";
export function segmentForFps(fps: number): FpsSegment {
  return fps === 30 || fps === 60 ? fps : "custom";
}

/** Positive UI value <-> negative stored flag. Symmetric. */
export const invert = (b: boolean): boolean => !b;
export const playsAudio = (s: Pick<CoreSettings, "noAudio">) => !s.noAudio;
export const allowsControl = (s: Pick<CoreSettings, "noControl">) => !s.noControl;
export const usesOnScreenKeyboard = (s: Pick<CoreSettings, "noKeyboardIme">) => !s.noKeyboardIme;
export const keepsAspectRatio = (s: Pick<CoreSettings, "noWindowAspectRatioLock">) =>
  !s.noWindowAspectRatioLock;
