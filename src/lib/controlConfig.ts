
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
  | "swipeScroll";

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
  },
};

export { loadControlConfig, saveControlConfig } from "./settingsStore";
