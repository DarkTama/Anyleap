import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/persist", async (orig) => ({
  ...(await orig<typeof import("@/lib/persist")>()),
  saveModeDefaults: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(async () => {}),
  listen: vi.fn(async () => () => {}),
}));

vi.mock("@/lib/tauri", () => ({
  getScrollMultiplier: vi.fn(async () => 2.75),
  setScrollFeel: vi.fn(async () => null),
  showMainWindow: vi.fn(async () => {}),
  getPhoneToggles: vi.fn(async () => ({ wifi: false, bluetooth: false, dnd: false })),
  getCallActive: vi.fn(async () => false),
}));

import { ScrollPopover } from "./ScrollPopover";
import { ControlBar } from "./ControlBar";
import { DEFAULT_CONTROL_CONFIG } from "@/lib/controlConfig";
import { defaultModeDefaults } from "@/lib/modeSettings";
import * as modeActions from "@/lib/modeActions";
import { getScrollMultiplier, setScrollFeel } from "@/lib/tauri";
import { useAppStore } from "@/store/useAppStore";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

describe("ScrollPopover", () => {
  let host: HTMLDivElement;
  let root: Root;
  const onOpenSettings = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    useAppStore.setState({
      modeDefaults: defaultModeDefaults(),
      sessions: [{ id: "s1", serial: "DEV1", pid: 100, started_at: 1, mode: "display" }],
      sessionModes: { s1: "flex" },
      currentMode: "flex",
    });
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });

  const scrollBtn = () => host.querySelector<HTMLButtonElement>('button[aria-label="Scroll"]')!;
  const dialog = () => document.querySelector<HTMLElement>('[role="dialog"][aria-label="Scroll settings"]');
  const radio = (label: string) =>
    Array.from(document.querySelectorAll<HTMLButtonElement>('[role="dialog"] [role="radio"]')).find(
      (b) => b.textContent === label,
    )!;
  const smoothSwitch = () =>
    document.querySelector<HTMLButtonElement>('[role="dialog"] [role="switch"]')!;
  const selectByLabel = (labelText: string) => {
    const lbl = Array.from(document.querySelectorAll<HTMLLabelElement>('[role="dialog"] label')).find(
      (l) => l.textContent === labelText,
    )!;
    return document.querySelector<HTMLSelectElement>(`#${lbl.htmlFor}`)!;
  };
  const key = async (target: Element, k: string, shiftKey = false) => {
    await act(async () => {
      target.dispatchEvent(
        new KeyboardEvent("keydown", { key: k, shiftKey, bubbles: true, cancelable: true }),
      );
    });
  };

  it("renders Scroll trigger with a mouse SVG and opens popover with all controls + live Auto multiplier", async () => {
    await act(async () => {
      root.render(<ScrollPopover serial="DEV1" mode="flex" onOpenSettings={onOpenSettings} />);
    });

    const btn = scrollBtn();
    expect(btn).not.toBeNull();
    expect(btn.querySelector("svg")).not.toBeNull();
    expect(btn.getAttribute("aria-expanded")).toBe("false");

    await act(async () => btn.click());

    expect(btn.getAttribute("aria-expanded")).toBe("true");
    const panel = dialog()!;
    expect(panel).not.toBeNull();
    expect(getScrollMultiplier).toHaveBeenCalledWith("DEV1");
    expect(radio("Smooth (reading)").getAttribute("aria-checked")).toBe("true");
    expect(radio("Paged (short videos)").getAttribute("aria-checked")).toBe("false");

    const speedSel = selectByLabel("Scroll speed");
    expect(speedSel.value).toBe("auto");
    expect(speedSel.options[0].textContent).toBe("Auto · 2.75x");
    expect(speedSel.disabled).toBe(false);

    expect(smoothSwitch().getAttribute("aria-checked")).toBe("true");
    expect(smoothSwitch().disabled).toBe(false);

    const touchpadSel = selectByLabel("Touchpad speed");
    expect(touchpadSel.value).toBe("1");
    expect(touchpadSel.disabled).toBe(false);

    expect(panel.textContent).toContain("Saved for Flex");
    expect(panel.textContent).toContain("All settings");
  });

  it("disables speed, smooth, and touchpad under Paged with Settings note and calls editMode for the active mode only", async () => {
    const editSpy = vi.spyOn(modeActions, "editMode");
    await act(async () => {
      root.render(<ScrollPopover serial="DEV1" mode="flex" onOpenSettings={onOpenSettings} />);
    });
    await act(async () => scrollBtn().click());

    await act(async () => radio("Paged (short videos)").click());

    expect(editSpy).toHaveBeenCalledWith("flex", {
      scroll: { ...defaultModeDefaults().flex.scroll, style: "paged" },
    });
    expect(useAppStore.getState().modeDefaults.flex.scroll.style).toBe("paged");
    expect(useAppStore.getState().modeDefaults.mirror.scroll.style).toBe("smooth");
    expect(setScrollFeel).toHaveBeenCalledWith("DEV1", {
      style: "paged",
      speedMode: "auto",
      manual: 1,
      touchpadSpeed: 1,
      smooth: true,
      inertia: "off",
    });

    expect(selectByLabel("Scroll speed").disabled).toBe(true);
    expect(smoothSwitch().disabled).toBe(true);
    expect(selectByLabel("Touchpad speed").disabled).toBe(true);
    expect(dialog()!.textContent).toContain("Not used in Paged style.");

    editSpy.mockRestore();
  });

  it("reflects external modeDefaults changes (Settings -> Popover) and invokes All settings link", async () => {
    await act(async () => {
      root.render(<ScrollPopover serial="DEV1" mode="mirror" onOpenSettings={onOpenSettings} />);
    });
    await act(async () => scrollBtn().click());

    expect(dialog()!.textContent).toContain("Saved for Mirror");

    await act(async () => {
      modeActions.editMode("mirror", {
        scroll: { ...defaultModeDefaults().mirror.scroll, speed: 3, smooth: false, touchpadSpeed: 1.5 },
      });
    });

    expect(selectByLabel("Scroll speed").value).toBe("3");
    expect(smoothSwitch().getAttribute("aria-checked")).toBe("false");
    expect(selectByLabel("Touchpad speed").value).toBe("1.5");

    const allSettings = Array.from(dialog()!.querySelectorAll("button")).find(
      (b) => b.textContent === "All settings",
    )!;
    await act(async () => allSettings.click());
    expect(onOpenSettings).toHaveBeenCalledWith("mirror");
    expect(dialog()).toBeNull();
  });

  it("supports full keyboard operation: focus trap, Escape closes & restores focus, and arrows on Scroll style", async () => {
    await act(async () => {
      root.render(<ScrollPopover serial="DEV1" mode="flex" onOpenSettings={onOpenSettings} />);
    });
    await act(async () => scrollBtn().click());

    const firstRadio = radio("Smooth (reading)");
    expect(document.activeElement).toBe(firstRadio);

    // Arrow key moves within the segmented control and switches to Paged
    await key(firstRadio, "ArrowRight");
    expect(useAppStore.getState().modeDefaults.flex.scroll.style).toBe("paged");

    // ArrowLeft moves back to Smooth
    await key(radio("Paged (short videos)"), "ArrowLeft");
    expect(useAppStore.getState().modeDefaults.flex.scroll.style).toBe("smooth");

    // Focus trap: Shift+Tab from first control wraps to last ("All settings")
    const allSettingsBtn = Array.from(dialog()!.querySelectorAll("button")).find(
      (b) => b.textContent === "All settings",
    )!;
    firstRadio.focus();
    await key(firstRadio, "Tab", true);
    expect(document.activeElement).toBe(allSettingsBtn);

    // Tab from last wraps back to first
    await key(allSettingsBtn, "Tab", false);
    expect(document.activeElement).toBe(radio("Smooth (reading)"));

    // Escape closes and returns focus to Scroll button
    await key(document.activeElement!, "Escape");
    expect(dialog()).toBeNull();
    expect(document.activeElement).toBe(scrollBtn());
  });

  it("ControlBar shows Scroll in display mode, hides it in Camera, and removes Paged scrolling from More", async () => {
    await act(async () => {
      root.render(
        <ControlBar
          serial="DEV1"
          config={DEFAULT_CONTROL_CONFIG}
          orientation="vertical"
          sessionMode="display"
          displayMode="flex"
        />,
      );
    });
    expect(host.querySelector('button[aria-label="Scroll"]')).not.toBeNull();

    const moreBtn = host.querySelector<HTMLButtonElement>('button[aria-label="More"]')!;
    await act(async () => moreBtn.click());
    const morePanel = document.querySelector('[aria-label="More controls"]')!;
    expect(morePanel.textContent).not.toContain("Paged scrolling");

    await act(async () => {
      root.render(
        <ControlBar
          serial="DEV1"
          config={DEFAULT_CONTROL_CONFIG}
          orientation="vertical"
          sessionMode="camera"
        />,
      );
    });
    expect(host.querySelector('button[aria-label="Scroll"]')).toBeNull();
  });
});
