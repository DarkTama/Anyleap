
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
  | "sleep"
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
  "sleep",
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
  recents: "Recents",
  volUp: "Volume +",
  volDown: "Volume −",
  power: "Power",
  screenshot: "Screenshot",
  notifications: "Notifications",
  sleep: "Sleep / Wake",
  screenOff: "Screen off (scrcpy)",
  orientToggle: "Rotate device",
  swipeScroll: "Swipe scroll (Reels)",
  record: "Record screen",
  wifi: "Wi-Fi toggle",
  bluetooth: "Bluetooth toggle",
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
    sleep: true,
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

export { loadControlConfig, saveControlConfig } from "./settingsStore";
