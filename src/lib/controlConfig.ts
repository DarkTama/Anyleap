
export type DockSide = "left" | "right" | "top" | "bottom" | "undocked";
export type ControlSize = "sm" | "md" | "lg";
export type ButtonId =
  | "back"
  | "home"
  | "recents"
  | "volUp"
  | "volDown"
  | "power"
  | "screenshot"
  | "notifications"
  | "screenOff"
  | "orientToggle"
  | "swipeScroll"
  | "record"
  | "wifi"
  | "bluetooth"
  | "dnd"
  | "endCall";

export interface ControlConfig {
  dock: DockSide;
  size: ControlSize;
  buttons: Record<ButtonId, boolean>;
  /** Strip collapsed into the small round floating button. */
  collapsed: boolean;
}

export const BUTTON_IDS: ButtonId[] = [
  "back",
  "home",
  "recents",
  "volUp",
  "volDown",
  "power",
  "screenshot",
  "notifications",
  "screenOff",
  "orientToggle",
  "swipeScroll",
  "record",
  "wifi",
  "bluetooth",
  "dnd",
  "endCall",
];

export const BUTTON_LABELS: Record<ButtonId, string> = {
  back: "Back",
  home: "Home",
  recents: "Recent apps",
  volUp: "Volume up",
  volDown: "Volume down",
  power: "Power",
  screenshot: "Screenshot",
  notifications: "Notifications",
  screenOff: "Turn phone screen off (keep mirroring)",
  orientToggle: "Rotate",
  swipeScroll: "Mouse wheel swipes",
  record: "Record screen",
  wifi: "Wi-Fi",
  bluetooth: "Bluetooth",
  dnd: "Do Not Disturb",
  endCall: "End call",
};

export const DOCK_SIDES: DockSide[] = ["left", "right", "top", "bottom", "undocked"];
export const CONTROL_SIZES: ControlSize[] = ["sm", "md", "lg"];

export const DEFAULT_CONTROL_CONFIG: ControlConfig = {
  dock: "right",
  size: "md",
  collapsed: false,
  buttons: {
    back: true,
    home: true,
    recents: true,
    volUp: true,
    volDown: true,
    power: true,
    screenshot: true,
    notifications: true,
    screenOff: true,
    orientToggle: true,
    swipeScroll: true,
    record: true,
    wifi: true,
    bluetooth: true,
    dnd: true,
    endCall: true,
  },
};

/** Merge a stored control-bar config with defaults. Drops buttons that no
 *  longer exist (e.g. the removed `sleep` button, SPEC-28 phase 3). */
export function normalizeControlConfig(saved: unknown): ControlConfig {
  if (!saved || typeof saved !== "object") return DEFAULT_CONTROL_CONFIG;
  const s = saved as Partial<ControlConfig> & { buttons?: Record<string, unknown> };
  const buttons = { ...DEFAULT_CONTROL_CONFIG.buttons };
  for (const id of BUTTON_IDS) {
    const v = s.buttons?.[id];
    if (typeof v === "boolean") buttons[id] = v;
  }
  return {
    dock: s.dock ?? DEFAULT_CONTROL_CONFIG.dock,
    size: s.size ?? DEFAULT_CONTROL_CONFIG.size,
    buttons,
    collapsed: s.collapsed ?? DEFAULT_CONTROL_CONFIG.collapsed,
  };
}

export { loadControlConfig, saveControlConfig } from "./settingsStore";
