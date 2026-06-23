// Shared shapes between the React frontend and the Rust backend.

export type DeviceState =
  | "device"
  | "unauthorized"
  | "offline"
  | "no permissions"
  | "authorizing"
  | "bootloader"
  | "recovery"
  | "sideload"
  | (string & {});

export interface DeviceInfo {
  serial: string;
  state: DeviceState;
  model: string | null;
  product: string | null;
}

export interface SessionInfo {
  id: string;
  serial: string;
  pid: number;
  started_at: number;
}

export interface SessionExited {
  id: string;
  code: number | null;
  signal: number | null;
  last_error: string;
}

export type Codec = "h264" | "h265" | "av1";

export interface CoreSettings {
  maxSize: number; // --max-size; 0 = native
  videoBitRate: number; // bits/sec
  maxFps: number;
  videoCodec: Codec;
  stayAwake: boolean;
  turnScreenOff: boolean;
  fullscreen: boolean;
  showTouches: boolean;
  noAudio: boolean;
  noControl: boolean;
}

export type QualityPreset = "low" | "medium" | "high" | "highest" | "custom";

/** The fidelity/performance knobs a quality preset controls. */
export type PresetVideo = Pick<
  CoreSettings,
  "maxSize" | "videoBitRate" | "maxFps" | "videoCodec"
>;

export const PRESET_BUNDLES: Record<Exclude<QualityPreset, "custom">, PresetVideo> = {
  low: { maxSize: 800, videoBitRate: 2_000_000, maxFps: 30, videoCodec: "h264" },
  medium: { maxSize: 1280, videoBitRate: 8_000_000, maxFps: 60, videoCodec: "h264" },
  high: { maxSize: 1600, videoBitRate: 16_000_000, maxFps: 60, videoCodec: "h265" },
  highest: { maxSize: 0, videoBitRate: 30_000_000, maxFps: 60, videoCodec: "h265" },
};

export const PRESET_LABELS: Record<QualityPreset, string> = {
  low: "Low — Smooth",
  medium: "Medium — Balanced",
  high: "High",
  highest: "Highest — Crisp",
  custom: "Custom",
};

export const PRESET_HINTS: Record<QualityPreset, string> = {
  low: "Weak/busy Wi-Fi, lowest latency, older phones",
  medium: "Most users on decent Wi-Fi",
  high: "Strong 5 GHz Wi-Fi or USB; sharper text",
  highest: "USB or excellent Wi-Fi; max fidelity",
  custom: "Your own tuned settings",
};

export const DEFAULT_SETTINGS: CoreSettings = {
  ...PRESET_BUNDLES.medium,
  stayAwake: true,
  turnScreenOff: false,
  fullscreen: false,
  showTouches: false,
  noAudio: false,
  noControl: false,
};

// --- Wireless (M2) ---

export interface MdnsService {
  name: string;
  serviceType: string; // "_adb-tls-pairing._tcp" | "_adb-tls-connect._tcp" | "_adb._tcp"
  host: string;
  port: number;
}

export interface SavedDevice {
  id: string; // pragmatic key = host (M2)
  label: string;
  host: string;
  port: number; // last-known connect port
  lastSerial: string; // "host:port" as last seen by listDevices
}

export interface MirrorRect {
  x: number;
  y: number;
  width: number;
  height: number;
  minimized: boolean;
  workLeft: number;
  workTop: number;
  workRight: number;
  workBottom: number;
}
