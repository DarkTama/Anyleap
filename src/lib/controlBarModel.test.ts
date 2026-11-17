import { describe, expect, it } from "vitest";
import { DEFAULT_CONTROL_CONFIG, BUTTON_IDS, normalizeControlConfig } from "./controlConfig";
import {
  CONTROL_SETTINGS_GROUPS,
  DEFAULT_CONTROL_BAR_STATE,
  controlBarGroups,
  type ControlBarState,
} from "./controlBarModel";

const st = (over: Partial<ControlBarState> = {}): ControlBarState => ({ ...DEFAULT_CONTROL_BAR_STATE, ...over });
const ids = (m: ReturnType<typeof controlBarGroups>) => m.groups.map((g) => [g.id, g.items.map((i) => i.id)]);

describe("controlBarGroups", () => {
  it("orders groups Navigate, Sound, Screen, Capture", () => {
    const m = controlBarGroups(DEFAULT_CONTROL_CONFIG, "display", st());
    expect(ids(m)).toEqual([
      ["navigate", ["back", "home", "recents"]],
      ["sound", ["volUp", "volDown"]],
      ["screen", ["power", "screenOff", "orientToggle"]],
      ["capture", ["screenshot", "record"]],
    ]);
  });

  it("has no Sleep button anywhere", () => {
    const m = controlBarGroups(DEFAULT_CONTROL_CONFIG, "display", st({ inCall: true }));
    const all = [...m.groups.flatMap((g) => g.items), ...m.more].map((i) => i.id as string);
    expect(all).not.toContain("sleep");
    expect((BUTTON_IDS as string[]).includes("sleep")).toBe(false);
  });

  it("shows End call only during a call, in danger style", () => {
    const idle = controlBarGroups(DEFAULT_CONTROL_CONFIG, "display", st());
    expect(idle.groups.flatMap((g) => g.items).some((i) => i.id === "endCall")).toBe(false);
    const live = controlBarGroups(DEFAULT_CONTROL_CONFIG, "display", st({ inCall: true }));
    const end = live.groups.flatMap((g) => g.items).find((i) => i.id === "endCall");
    expect(end?.danger).toBe(true);
    const off = controlBarGroups(
      { ...DEFAULT_CONTROL_CONFIG, buttons: { ...DEFAULT_CONTROL_CONFIG.buttons, endCall: false } },
      "display",
      st({ inCall: true }),
    );
    expect(off.groups.flatMap((g) => g.items).some((i) => i.id === "endCall")).toBe(false);
  });

  it("marks toggles with pressed state and red recording", () => {
    const m = controlBarGroups(DEFAULT_CONTROL_CONFIG, "display", st({ isRecording: true, screenOff: true }));
    const items = m.groups.flatMap((g) => g.items);
    const rec = items.find((i) => i.id === "record");
    expect(rec).toMatchObject({ kind: "toggle", pressed: true, danger: true, label: "Stop recording" });
    expect(items.find((i) => i.id === "screenOff")).toMatchObject({ kind: "toggle", pressed: true });
  });

  it("hides Rotate in flex and keeps More contents in order", () => {
    const m = controlBarGroups(DEFAULT_CONTROL_CONFIG, "display", st({ showOrient: false }));
    expect(m.groups.flatMap((g) => g.items).some((i) => i.id === "orientToggle")).toBe(false);
    expect(m.more.map((i) => i.id)).toEqual(["notifications", "wifi", "bluetooth", "dnd", "swipeScroll"]);
    expect(m.more.find((i) => i.id === "swipeScroll")?.hint).toMatch(/short-video/);
    const noSwipe = controlBarGroups(DEFAULT_CONTROL_CONFIG, "display", st({ showSwipe: false }));
    expect(noSwipe.more.map((i) => i.id)).not.toContain("swipeScroll");
  });

  it("camera mode has a camera group (torch, zoom, stop) and no More", () => {
    const m = controlBarGroups(DEFAULT_CONTROL_CONFIG, "camera", st({ torchOn: true }));
    expect(m.groups[0].id).toBe("camera");
    const cam = m.groups[0].items.map((i) => i.id);
    expect(cam).toEqual(expect.arrayContaining(["torch", "zoomIn", "zoomOut", "stopCamera"]));
    expect(m.groups[0].items.find((i) => i.id === "torch")?.pressed).toBe(true);
    expect(m.more).toEqual([]);
    expect(m.groups.flatMap((g) => g.items).some((i) => i.id === "back")).toBe(false);
  });

  it("collapsed floating surface shows Navigate and More only", () => {
    const m = controlBarGroups({ ...DEFAULT_CONTROL_CONFIG, collapsed: true }, "display", st(), "floating");
    expect(m.groups.map((g) => g.id)).toEqual(["navigate"]);
    expect(m.more.length).toBeGreaterThan(0);
  });

  it("collapsed embedded surface still shows every enabled group", () => {
    const open = controlBarGroups(DEFAULT_CONTROL_CONFIG, "display", st(), "embedded");
    const m = controlBarGroups({ ...DEFAULT_CONTROL_CONFIG, collapsed: true }, "display", st(), "embedded");
    expect(m).toEqual(open);
    expect(m.groups.length).toBeGreaterThan(1);
  });

  it("settings groups cover every button exactly once", () => {
    const listed = CONTROL_SETTINGS_GROUPS.flatMap((g) => g.buttons).sort();
    expect(listed).toEqual([...BUTTON_IDS].sort());
  });

  it("uses no third-party names in help copy", () => {
    const m = controlBarGroups(DEFAULT_CONTROL_CONFIG, "camera", st());
    const help = m.groups[0].items.find((i) => i.id === "cameraHelp");
    expect(help?.hint).toBe("Use AnyLeap Camera in any video-call or streaming app");
  });
});

describe("normalizeControlConfig", () => {
  it("drops the removed sleep button and keeps user choices", () => {
    const out = normalizeControlConfig({
      dock: "left",
      size: "lg",
      collapsed: true,
      buttons: { sleep: true, back: false },
    });
    expect("sleep" in out.buttons).toBe(false);
    expect(out.buttons.back).toBe(false);
    expect(out.buttons.home).toBe(true);
    expect(out).toMatchObject({ dock: "left", size: "lg", collapsed: true });
  });

  it("falls back to defaults for empty input", () => {
    expect(normalizeControlConfig(undefined)).toEqual(DEFAULT_CONTROL_CONFIG);
    expect(normalizeControlConfig(null)).toEqual(DEFAULT_CONTROL_CONFIG);
  });
});
