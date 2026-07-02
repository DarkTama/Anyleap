import { load, type Store } from "@tauri-apps/plugin-store";

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

const FILE = "config.json";
const KEY = "controlBar";
let storePromise: Promise<Store> | null = null;
const getStore = () => (storePromise ??= load(FILE, { autoSave: true, defaults: {} }));

export async function loadControlConfig(): Promise<ControlConfig> {
  const store = await getStore();
  const saved = await store.get<Partial<ControlConfig>>(KEY);
  if (!saved) return DEFAULT_CONTROL_CONFIG;
  return {
    dock: saved.dock ?? DEFAULT_CONTROL_CONFIG.dock,
    size: saved.size ?? DEFAULT_CONTROL_CONFIG.size,
    buttons: { ...DEFAULT_CONTROL_CONFIG.buttons, ...(saved.buttons ?? {}) },
    collapsed: saved.collapsed ?? DEFAULT_CONTROL_CONFIG.collapsed,
  };
}

export async function saveControlConfig(config: ControlConfig): Promise<void> {
  const store = await getStore();
  await store.set(KEY, config);
  await store.save();
}
