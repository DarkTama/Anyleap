import type { ButtonId, ControlConfig } from "./controlConfig";
import type { SessionMode } from "./types";

/** Visual group ids, in top-to-bottom order (SPEC-28 phase 3, SPEC-30 addendum). */
export type ControlGroupId = "camera" | "navigate" | "sound" | "screen" | "capture" | "scroll" | "call";

/** Settings-card group ids: the bar groups plus the More popover. */
export type ControlSettingsGroupId = Exclude<ControlGroupId, "camera"> | "more";

export type ControlItemId =
  | ButtonId
  | "torch"
  | "zoomIn"
  | "zoomOut"
  | "cameraHelp"
  | "stopCamera";

export type ControlItemKind = "action" | "toggle";

export interface ControlItem {
  id: ControlItemId;
  /** Accessible name and tooltip text. */
  label: string;
  kind: ControlItemKind;
  /** Only meaningful for toggles; drives `aria-pressed` and the gold style. */
  pressed?: boolean;
  /** Danger color (End call, active recording). */
  danger?: boolean;
  /** Longer tooltip detail shown under the label. */
  hint?: string;
}

export interface ControlGroup {
  id: ControlGroupId;
  label: string;
  items: ControlItem[];
}

export interface ControlBarState {
  inCall: boolean;
  isRecording: boolean;
  screenOff: boolean;
  torchOn: boolean;
  wifi: boolean;
  bluetooth: boolean;
  dnd: boolean;
  /** Rotate acts on the physical display only: false in Flex display mode. */
  showOrient: boolean;
  /** Scroll quick-settings popover applies to mirroring displays, not Camera. */
  showScroll: boolean;
}

export const DEFAULT_CONTROL_BAR_STATE: ControlBarState = {
  inCall: false,
  isRecording: false,
  screenOff: false,
  torchOn: false,
  wifi: false,
  bluetooth: false,
  dnd: false,
  showOrient: true,
  showScroll: true,
};

export const CONTROL_GROUP_LABELS: Record<ControlSettingsGroupId, string> = {
  navigate: "Navigate",
  sound: "Sound",
  screen: "Screen",
  capture: "Capture",
  scroll: "Scroll",
  call: "Call",
  more: "More",
};

/** Which configurable buttons belong to which group (used by the Settings card). */
export const CONTROL_SETTINGS_GROUPS: { id: ControlSettingsGroupId; label: string; buttons: ButtonId[] }[] = [
  { id: "navigate", label: CONTROL_GROUP_LABELS.navigate, buttons: ["back", "home", "recents"] },
  { id: "sound", label: CONTROL_GROUP_LABELS.sound, buttons: ["volUp", "volDown"] },
  { id: "screen", label: CONTROL_GROUP_LABELS.screen, buttons: ["power", "screenOff", "orientToggle"] },
  { id: "capture", label: CONTROL_GROUP_LABELS.capture, buttons: ["screenshot", "record"] },
  { id: "scroll", label: CONTROL_GROUP_LABELS.scroll, buttons: ["scroll"] },
  { id: "call", label: CONTROL_GROUP_LABELS.call, buttons: ["endCall"] },
  { id: "more", label: CONTROL_GROUP_LABELS.more, buttons: ["notifications", "wifi", "bluetooth", "dnd"] },
];

export const CAMERA_HELP_HINT = "Use AnyLeap Camera in any video-call or streaming app";

export interface ControlBarModel {
  groups: ControlGroup[];
  /** Items inside the More popover. */
  more: ControlItem[];
}

/** Where the bar renders. `collapsed` applies to the floating window only. */
export type ControlSurface = "embedded" | "floating";

/** Pure, grouped button model. Order and visibility rules live here, not in JSX. */
export function controlBarGroups(
  config: ControlConfig,
  sessionMode: SessionMode,
  state: ControlBarState = DEFAULT_CONTROL_BAR_STATE,
  surface: ControlSurface = "embedded",
): ControlBarModel {
  const b = config.buttons;
  const camera = sessionMode === "camera";
  const groups: ControlGroup[] = [];
  const add = (id: ControlGroupId, items: (ControlItem | false)[]) => {
    const kept = items.filter((i): i is ControlItem => !!i);
    if (kept.length > 0) groups.push({ id, label: CONTROL_GROUP_LABELS[id as ControlSettingsGroupId] ?? "Camera", items: kept });
  };

  const screenItems = (): (ControlItem | false)[] => [
    !camera && b.power && { id: "power", label: "Power", kind: "action" },
    b.screenOff && {
      id: "screenOff",
      label: "Turn phone screen off (keep mirroring)",
      kind: "toggle",
      pressed: state.screenOff,
    },
    b.orientToggle && state.showOrient && { id: "orientToggle", label: "Rotate", kind: "action" },
  ];

  if (camera) {
    add("camera", [
      { id: "torch", label: "Torch", kind: "toggle", pressed: state.torchOn },
      { id: "zoomIn", label: "Zoom in", kind: "action" },
      { id: "zoomOut", label: "Zoom out", kind: "action" },
      { id: "cameraHelp", label: "Camera help", kind: "action", hint: CAMERA_HELP_HINT },
      { id: "stopCamera", label: "Stop camera", kind: "action", danger: true },
    ]);
    add("screen", screenItems());
  } else {
    add("navigate", [
      b.back && { id: "back", label: "Back", kind: "action" },
      b.home && { id: "home", label: "Home", kind: "action" },
      b.recents && { id: "recents", label: "Recent apps", kind: "action" },
    ]);
    add("sound", [
      b.volUp && { id: "volUp", label: "Volume up", kind: "action" },
      b.volDown && { id: "volDown", label: "Volume down", kind: "action" },
    ]);
    add("screen", screenItems());
    add("capture", [
      b.screenshot && { id: "screenshot", label: "Screenshot", kind: "action" },
      b.record && {
        id: "record",
        label: state.isRecording ? "Stop recording" : "Record screen",
        kind: "toggle",
        pressed: state.isRecording,
        danger: state.isRecording,
      },
    ]);
    add("scroll", [b.scroll && state.showScroll && { id: "scroll", label: "Scroll", kind: "action" }]);
    add("call", [b.endCall && state.inCall && { id: "endCall", label: "End call", kind: "action", danger: true }]);
  }

  const more: ControlItem[] = camera
    ? []
    : ([
        b.notifications && { id: "notifications", label: "Notifications", kind: "action" },
        b.wifi && { id: "wifi", label: "Wi-Fi", kind: "toggle", pressed: state.wifi },
        b.bluetooth && { id: "bluetooth", label: "Bluetooth", kind: "toggle", pressed: state.bluetooth },
        b.dnd && { id: "dnd", label: "Do Not Disturb", kind: "toggle", pressed: state.dnd },
      ].filter(Boolean) as ControlItem[]);

  // Collapsed mode (floating control window only): Navigate and More only.
  // The embedded mirror sidebar always shows every enabled group.
  if (surface === "floating" && config.collapsed) {
    return { groups: groups.filter((g) => g.id === "navigate"), more };
  }
  return { groups, more };
}
