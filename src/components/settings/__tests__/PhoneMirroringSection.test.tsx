import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/persist", async (orig) => ({
  ...(await orig<typeof import("@/lib/persist")>()),
  saveModeDefaults: vi.fn(async () => {}),
  saveModeDefaultsNotice: vi.fn(async () => {}),
}));
vi.mock("@/lib/tauri", () => ({
  getSystemResolution: vi.fn(async () => [1920, 1080]),
  getScrollMultiplier: vi.fn(async () => null),
  setScrollFeel: vi.fn(async () => null),
}));

import { PhoneMirroringSection } from "../PhoneMirroringSection";
import { useAppStore } from "@/store/useAppStore";
import { defaultModeDefaults } from "@/lib/modeSettings";
import { saveModeDefaults } from "@/lib/persist";
import { getScrollMultiplier, setScrollFeel } from "@/lib/tauri";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
let el: HTMLDivElement;

async function mount() {
  el = document.createElement("div");
  document.body.appendChild(el);
  root = createRoot(el);
  await act(async () => {
    root.render(<PhoneMirroringSection />);
  });
}
const tab = (name: string) =>
  Array.from(el.querySelectorAll<HTMLElement>('[role="tab"]')).find((t) => t.textContent === name)!;
const sw = (label: string) =>
  Array.from(el.querySelectorAll<HTMLElement>('[role="switch"]')).find(
    (b) => document.getElementById(b.getAttribute("aria-labelledby")!)?.textContent === label,
  )!;

describe("PhoneMirroringSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAppStore.setState({
      modeDefaults: defaultModeDefaults(),
      modeDefaultsNotice: false,
      currentMode: "flex",
    });
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    el.remove();
  });

  it("renders four mode tabs; selecting a tab writes no settings", async () => {
    await mount();
    const names = Array.from(el.querySelectorAll('[role="tab"]')).map((t) => t.textContent);
    expect(names).toEqual(["Mirror", "Flex", "Camera", "High fidelity"]);
    const before = structuredClone(useAppStore.getState().modeDefaults);
    for (const n of ["Flex", "Camera", "High fidelity", "Mirror"]) {
      await act(async () => tab(n).click());
    }
    expect(useAppStore.getState().modeDefaults).toEqual(before);
    expect(saveModeDefaults).not.toHaveBeenCalled();
    expect(useAppStore.getState().currentMode).toBe("flex");
  });

  it("opens the requested mode tab when targetMode updates", async () => {
    el = document.createElement("div");
    document.body.appendChild(el);
    root = createRoot(el);
    await act(async () => {
      root.render(<PhoneMirroringSection targetMode={{ mode: "flex", tick: 1 }} />);
    });
    expect(tab("Flex").getAttribute("aria-selected")).toBe("true");
  });

  it("Camera then Mirror leaves Mirror audio unchanged", async () => {
    await mount();
    await act(async () => tab("Camera").click());
    await act(async () => sw("Use the phone microphone on this PC").click());
    expect(useAppStore.getState().modeDefaults.camera.playAudio).toBe(false);
    await act(async () => tab("Mirror").click());
    expect(useAppStore.getState().modeDefaults.mirror.noAudio).toBe(false);
    expect(sw("Play phone audio on this PC").getAttribute("aria-checked")).toBe("true");
  });

  it("the audio switch writes the inverted flag to the selected mode only", async () => {
    await mount();
    await act(async () => tab("High fidelity").click());
    const before = structuredClone(useAppStore.getState().modeDefaults);
    await act(async () => sw("Play phone audio on this PC").click());
    const after = useAppStore.getState().modeDefaults;
    expect(after.hifi.noAudio).toBe(true);
    expect(after.mirror).toEqual(before.mirror);
    expect(after.flex).toEqual(before.flex);
    expect(after.camera).toEqual(before.camera);
    expect(saveModeDefaults).toHaveBeenCalledTimes(1);
  });

  it("Flex display controls exist on the Flex tab only; no flex or preset toggles elsewhere", async () => {
    await mount();
    expect(el.textContent).not.toContain("Virtual display size");
    expect(el.textContent).not.toContain("Quality preset");
    expect(el.textContent).not.toContain("Flex display (desktop mode)");
    await act(async () => tab("Flex").click());
    expect(el.textContent).toContain("Virtual display size");
    expect(el.textContent).toContain("Hide Android taskbar");
    await act(async () => tab("High fidelity").click());
    expect(el.textContent).not.toContain("Virtual display size");
  });

  it("aspect-ratio switch is disabled with a reason while one-window mode is on", async () => {
    await mount();
    const row = sw("Keep phone aspect ratio");
    expect(row.hasAttribute("disabled")).toBe(true);
    expect(el.textContent).toContain("Not available while the phone is shown in one AnyLeap window.");
    await act(async () => sw("Show phone in one AnyLeap window").click());
    expect(sw("Keep phone aspect ratio").hasAttribute("disabled")).toBe(false);
  });

  it("on-screen keyboard switch is disabled with helper text on Flex, but toggles on Mirror and High fidelity", async () => {
    useAppStore.setState({
      modeDefaults: {
        ...defaultModeDefaults(),
        flex: { ...defaultModeDefaults().flex, noKeyboardIme: false },
      },
    });
    await mount();
    expect(sw("Use Android on-screen keyboard").hasAttribute("disabled")).toBe(false);
    await act(async () => sw("Use Android on-screen keyboard").click());
    expect(useAppStore.getState().modeDefaults.mirror.noKeyboardIme).toBe(true);

    await act(async () => tab("High fidelity").click());
    expect(sw("Use Android on-screen keyboard").hasAttribute("disabled")).toBe(false);
    await act(async () => sw("Use Android on-screen keyboard").click());
    expect(useAppStore.getState().modeDefaults.hifi.noKeyboardIme).toBe(true);

    const callsBeforeFlex = vi.mocked(saveModeDefaults).mock.calls.length;
    await act(async () => tab("Flex").click());
    const flexRow = sw("Use Android on-screen keyboard");
    expect(flexRow.hasAttribute("disabled")).toBe(true);
    expect(flexRow.getAttribute("aria-checked")).toBe("false");
    expect(el.textContent).toContain(
      "Not used in Flex. PC typing works, and the phone's keyboard may appear on the phone while you type in Flex.",
    );
    await act(async () => flexRow.click());
    expect(useAppStore.getState().modeDefaults.flex.noKeyboardIme).toBe(false);
    expect(vi.mocked(saveModeDefaults).mock.calls.length).toBe(callsBeforeFlex);
  });

  it("every switch has a visible label and the embedded helper names no product", async () => {
    await mount();
    for (const b of el.querySelectorAll<HTMLElement>('[role="switch"]')) {
      const label = document.getElementById(b.getAttribute("aria-labelledby")!);
      expect(label?.textContent?.length).toBeGreaterThan(0);
    }
    expect(el.textContent).toContain("Keeps the phone view and its toolbar together.");
  });

  it("shows the one-time audio notice and dismiss clears it", async () => {
    useAppStore.setState({ modeDefaultsNotice: true });
    await mount();
    expect(el.textContent).toContain(
      "Audio is now on in Mirror, Flex and High fidelity. Turn it off per mode here.",
    );
    const dismiss = Array.from(el.querySelectorAll("button")).find((b) => b.textContent === "Dismiss")!;
    await act(async () => dismiss.click());
    expect(useAppStore.getState().modeDefaultsNotice).toBe(false);
    expect(el.textContent).not.toContain("Audio is now on");
  });

  it("quality segments write only the selected mode and Custom shows for odd values", async () => {
    await mount();
    const sharp = Array.from(el.querySelectorAll<HTMLElement>('[role="radio"]')).find(
      (r) => r.textContent === "Sharp",
    )!;
    await act(async () => sharp.click());
    const d = useAppStore.getState().modeDefaults;
    expect(d.mirror.videoBitRate).toBe(16_000_000);
    expect(d.flex.videoBitRate).toBe(8_000_000);
    useAppStore.getState().updateMode("mirror", { maxFps: 45 });
    await act(async () => {});
    expect(el.textContent).toContain("Custom");
  });

  it("renders Scrolling group with style, speed, Touchpad speed, and Smooth scrolling switch, disables controls under Paged, and hides Phase 3", async () => {
    useAppStore.setState({ sessions: [], sessionModes: {} });
    await mount();
    expect(el.textContent).toContain("Scrolling");
    expect(el.textContent).toContain("Scroll style");
    expect(el.textContent).toContain("Smooth (reading)");
    expect(el.textContent).toContain("Paged (short videos)");
    expect(el.textContent).toContain("Scroll speed");
    expect(el.textContent).toContain("Matches the phone's normal feel.");
    expect(el.textContent).toContain("Touchpad speed");
    expect(el.textContent).toContain(
      "Touchpads already scroll precisely, so this has its own speed.",
    );
    expect(el.textContent).toContain("Smooth scrolling");
    expect(el.textContent).toContain(
      "Glides between wheel notches. Adds a little delay, so turn it off for games.",
    );
    expect(el.textContent).not.toContain("Touchpad momentum");

    const smoothSwitch = sw("Smooth scrolling");
    expect(smoothSwitch.getAttribute("aria-checked")).toBe("true");
    expect(smoothSwitch.hasAttribute("disabled")).toBe(false);

    // High fidelity defaults Smooth scrolling to off.
    await act(async () => tab("High fidelity").click());
    expect(sw("Smooth scrolling").getAttribute("aria-checked")).toBe("false");

    await act(async () => tab("Mirror").click());
    const speedSelect = Array.from(el.querySelectorAll<HTMLSelectElement>("select")).find((sel) =>
      Array.from(sel.options).some((o) => o.value === "auto"),
    )!;
    expect(speedSelect.value).toBe("auto");
    expect(speedSelect.disabled).toBe(false);

    const touchpadLabel = Array.from(el.querySelectorAll("label")).find(
      (l) => l.textContent === "Touchpad speed",
    )!;
    const touchpadSelect = el.querySelector<HTMLSelectElement>(`#${touchpadLabel.htmlFor}`)!;
    expect(touchpadSelect.value).toBe("1");
    expect(touchpadSelect.disabled).toBe(false);
    expect(Array.from(touchpadSelect.options).map((o) => o.value)).toEqual([
      "0.5",
      "0.75",
      "1",
      "1.25",
      "1.5",
      "1.75",
      "2",
      "2.25",
      "2.5",
      "2.75",
      "3",
    ]);

    const pagedBtn = Array.from(el.querySelectorAll<HTMLElement>('[role="radio"]')).find(
      (r) => r.textContent === "Paged (short videos)",
    )!;
    await act(async () => pagedBtn.click());
    expect(useAppStore.getState().modeDefaults.mirror.scroll.style).toBe("paged");
    expect(useAppStore.getState().modeDefaults.flex.scroll.style).toBe("smooth");
    expect(speedSelect.disabled).toBe(true);
    expect(touchpadSelect.disabled).toBe(true);
    expect(sw("Smooth scrolling").hasAttribute("disabled")).toBe(true);
    expect(el.textContent).toContain("Not used in Paged style.");
  });

  it("toggling Smooth scrolling and Touchpad speed writes only the selected mode and updates running sessions live", async () => {
    useAppStore.setState({
      sessions: [{ id: "s1", serial: "DEV1", pid: 1, started_at: 1, mode: "display" }],
      sessionModes: { s1: "mirror" },
      currentMode: "mirror",
    });
    await mount();
    await act(async () => sw("Smooth scrolling").click());
    expect(useAppStore.getState().modeDefaults.mirror.scroll.smooth).toBe(false);
    expect(useAppStore.getState().modeDefaults.flex.scroll.smooth).toBe(true);
    expect(setScrollFeel).toHaveBeenCalledWith("DEV1", {
      style: "smooth",
      speedMode: "auto",
      manual: 1,
      touchpadSpeed: 1,
      smooth: false,
      inertia: "off",
    });

    const touchpadLabel = Array.from(el.querySelectorAll("label")).find(
      (l) => l.textContent === "Touchpad speed",
    )!;
    const touchpadSelect = el.querySelector<HTMLSelectElement>(`#${touchpadLabel.htmlFor}`)!;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
      setter.call(touchpadSelect, "1.75");
      touchpadSelect.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(useAppStore.getState().modeDefaults.mirror.scroll.touchpadSpeed).toBe(1.75);
    expect(useAppStore.getState().modeDefaults.flex.scroll.touchpadSpeed).toBe(1);
    expect(setScrollFeel).toHaveBeenLastCalledWith("DEV1", {
      style: "smooth",
      speedMode: "auto",
      manual: 1,
      touchpadSpeed: 1.75,
      smooth: false,
      inertia: "off",
    });
  });

  it("shows live Auto multiplier when a session of the active mode is running", async () => {
    vi.mocked(getScrollMultiplier).mockResolvedValueOnce(2.5);
    useAppStore.setState({
      sessions: [{ id: "s1", serial: "DEV1", pid: 1, started_at: 1, mode: "display" }],
      sessionModes: { s1: "flex" },
      currentMode: "flex",
    });
    await mount();
    await act(async () => tab("Flex").click());
    expect(el.textContent).toContain("Auto matches the phone's normal feel. Currently 2.5x on this screen.");
  });
});
