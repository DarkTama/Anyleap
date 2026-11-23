import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/persist", async (orig) => ({
  ...(await orig<typeof import("@/lib/persist")>()),
  saveModeDefaults: vi.fn(async () => {}),
  saveModeDefaultsNotice: vi.fn(async () => {}),
  saveAppPrefs: vi.fn(async () => {}),
  saveTransferFolders: vi.fn(async () => {}),
}));
const mockSetTransferFoldersConfig = vi.fn(async () => {});
vi.mock("@/lib/tauri", () => ({
  getSystemResolution: vi.fn(async () => [1920, 1080]),
  setTransferFoldersConfig: (...args: unknown[]) => mockSetTransferFoldersConfig(...(args as [])),
}));
const mockOpenDialog = vi.fn(async () => null as string | null);
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => mockOpenDialog(...(args as [])),
}));
const mockRevealItemInDir = vi.fn(async () => {});
vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: (...args: unknown[]) => mockRevealItemInDir(...(args as [])),
}));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(async () => {}) }));
vi.mock("@/services/kvmService", async (orig) => ({
  ...(await orig<typeof import("@/services/kvmService")>()),
  kvmSetSyncSettings: vi.fn(async () => {}),
  kvmSetFilesSettings: vi.fn(async () => {}),
  kvmSetAudioSettings: vi.fn(async () => {}),
  kvmSetSessionSettings: vi.fn(async () => {}),
  kvmSetDebugLog: vi.fn(async () => {}),
  kvmGetDebugLogPath: vi.fn(async () => "C:\\logs\\kvm-debug.log"),
  kvmOpenDebugLogFolder: vi.fn(async () => {}),
}));

import { SettingsPanel } from "@/components/SettingsPanel";
import { useAppStore } from "@/store/useAppStore";
import { useKvmStore } from "@/stores/kvmStore";
import { DEFAULT_TRANSFER_FOLDERS } from "@/lib/transferFolders";
import { SECTIONS, entry } from "../registry";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
let el: HTMLDivElement;
const onOpenComputers = vi.fn();

async function mount() {
  el = document.createElement("div");
  document.body.appendChild(el);
  root = createRoot(el);
  await act(async () => {
    root.render(<SettingsPanel onOpenComputers={onOpenComputers} />);
  });
}
const railButtons = () =>
  Array.from(el.querySelectorAll<HTMLButtonElement>('nav[aria-label="Settings sections"] button'));
const rail = (name: string) => railButtons().find((b) => b.textContent?.startsWith(name))!;
const key = async (target: Element, k: string) => {
  await act(async () => {
    target.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
  });
};
const openSection = async (name: string) => {
  await act(async () => rail(name).click());
};
const sw = (label: string) =>
  Array.from(el.querySelectorAll<HTMLElement>('[role="switch"]')).find(
    (b) => document.getElementById(b.getAttribute("aria-labelledby")!)?.textContent === label,
  )!;
const setInput = async (input: HTMLInputElement, value: string) => {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
};
const typeFilter = async (value: string) => {
  const input = el.querySelector<HTMLInputElement>('[role="search"] input')!;
  await setInput(input, value);
  return input;
};
const rawKvm = () => useKvmStore.getState().settings as unknown as Record<string, unknown>;

describe("SettingsPanel rail and filter (SPEC-28 phase 2)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useKvmStore.getState().reset();
    useAppStore.getState().setTransferFolders({ ...DEFAULT_TRANSFER_FOLDERS });
    useAppStore.getState().setDefaultTransferFolders({
      defaultReceiveDir: "C:\\Users\\Tama\\Downloads\\AnyLeap",
      defaultPhoneUploadDir: "/sdcard/Download",
    });
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    el.remove();
  });

  it("renders the seven sections in order inside a labelled nav", async () => {
    await mount();
    expect(railButtons().map((b) => b.textContent)).toEqual(SECTIONS.map((s) => s.label));
    expect(rail("General").getAttribute("aria-current")).toBe("page");
    expect(el.querySelector('[role="search"] label')?.textContent).toBe("Filter settings");
    expect(el.textContent).toContain("Skip to settings");
  });

  it("uses roving focus: one tab stop, Up/Down/Home/End move focus", async () => {
    await mount();
    expect(railButtons().filter((b) => b.tabIndex === 0)).toHaveLength(1);
    rail("General").focus();
    await key(rail("General"), "ArrowDown");
    expect(document.activeElement).toBe(rail("Phone mirroring"));
    await key(document.activeElement!, "End");
    expect(document.activeElement).toBe(rail("Advanced"));
    await key(document.activeElement!, "ArrowDown");
    expect(document.activeElement).toBe(rail("Advanced"));
    await key(document.activeElement!, "Home");
    expect(document.activeElement).toBe(rail("General"));
    await key(document.activeElement!, "ArrowUp");
    expect(document.activeElement).toBe(rail("General"));
  });

  it("opening a section sets aria-current and focuses its heading", async () => {
    await mount();
    await openSection("Clipboard");
    expect(rail("Clipboard").getAttribute("aria-current")).toBe("page");
    expect(rail("General").getAttribute("aria-current")).toBeNull();
    expect(document.activeElement?.tagName).toBe("H2");
    expect(document.activeElement?.textContent).toBe("Clipboard & files");
  });

  it("filter narrows the rail and pane, shows counts, and Escape clears", async () => {
    await mount();
    const input = await typeFilter("mute");
    const names = railButtons().map((b) => b.textContent);
    expect(names.length).toBeGreaterThan(0);
    expect(names.length).toBeLessThan(SECTIONS.length);
    expect(names.some((n) => n?.startsWith("Audio, media & power"))).toBe(true);
    expect(el.querySelector('[aria-live="polite"]')?.textContent).toMatch(/match/);
    await key(input, "Escape");
    expect(railButtons()).toHaveLength(SECTIONS.length);
    expect(input.value).toBe("");
  });

  it("shows an empty state with a Clear button when nothing matches", async () => {
    await mount();
    await typeFilter("zzzqqq");
    expect(el.textContent).toContain("No settings match “zzzqqq”");
    const clear = Array.from(el.querySelectorAll("button")).find((b) => b.textContent === "Clear")!;
    await act(async () => clear.click());
    expect(railButtons()).toHaveLength(SECTIONS.length);
  });

  it("auto-expands Advanced while the filter matches one of its rows", async () => {
    await mount();
    await openSection("Advanced");
    const toggle = () => el.querySelector<HTMLButtonElement>("button[aria-controls]")!;
    expect(toggle().getAttribute("aria-expanded")).toBe("false");
    await typeFilter("diagnostics");
    expect(toggle().getAttribute("aria-expanded")).toBe("true");
  });

  it("filtering never changes stored values", async () => {
    await mount();
    const before = JSON.stringify(useKvmStore.getState().settings);
    await typeFilter("clipboard");
    await typeFilter("");
    expect(JSON.stringify(useKvmStore.getState().settings)).toBe(before);
  });

  it("has no legacy jargon labels", async () => {
    await mount();
    const seen: string[] = [];
    for (const s of SECTIONS) {
      await openSection(s.label);
      seen.push(el.textContent ?? "");
    }
    const all = seen.join("\n");
    for (const bad of ["HUD Pill", "Beacon", "Staging", "Prefetch", "KVM diagnostics"]) {
      expect(all).not.toContain(bad);
    }
  });
});

describe("relabelled controls keep writing the same stored key", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useKvmStore.getState().reset();
    useAppStore.getState().setTransferFolders({ ...DEFAULT_TRANSFER_FOLDERS });
    useAppStore.getState().setDefaultTransferFolders({
      defaultReceiveDir: "C:\\Users\\Tama\\Downloads\\AnyLeap",
      defaultPhoneUploadDir: "/sdcard/Download",
    });
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    el.remove();
  });

  const cases: [string, string, string][] = [
    ["Share copied text", "Clipboard", "syncClipboardText"],
    ["Share copied images", "Clipboard", "syncClipboardImage"],
    [
      "Drag and drop files between PCs (Experimental)",
      "Clipboard",
      "continuityDragDropExperimental",
    ],
    ["Copy and paste files between PCs", "Clipboard", "continuityFileClipboard"],
    ["Match pointer style on the Linked PC", "Clipboard", "continuityPointerScheme"],
    ["Play Linked PC audio on this PC", "Audio", "continuityAudioBridge"],
    [
      "Silence the Linked PC\u2019s speakers meanwhile".replace("\u2019", "'"),
      "Audio",
      "continuityMuteLinkedWhileBridging",
    ],
    ["Media keys control the PC that is playing", "Audio", "continuityMediaKeysFollowAudio"],
    ["Lock both PCs together", "Audio", "continuityLockTogether"],
    ["Sleep both PCs together", "Audio", "continuitySleepTogether"],
    ['Add "Open on Linked PC" to the right-click menu', "Audio", "continuityOpenOnLinkedMenu"],
    ["Show which PC has the cursor", "Shared", "indicatorHud"],
    ["Show cursor position in the layout map", "Shared", "indicatorCanvasDot"],
    ["Record a diagnostics log", "Advanced", "debugLog"],
  ];

  for (const [label, section, k] of cases) {
    it(`"${label}" toggles ${k}`, async () => {
      await mount();
      await openSection(section);
      const before = Boolean(rawKvm()[k]);
      await act(async () => sw(label).click());
      expect(rawKvm()[k]).toBe(!before);
    });
  }

  it("Badge position shows only while the cursor badge is on and writes indicatorHudCorner", async () => {
    await mount();
    await openSection("Shared");
    act(() => useKvmStore.getState().updateSettings({ indicatorHud: false }));
    expect(el.textContent).not.toContain("Badge position");
    await act(async () => sw("Show which PC has the cursor").click());
    const select = Array.from(el.querySelectorAll("select")).find(
      (s) => el.querySelector(`label[for="${s.id}"]`)?.textContent === "Badge position",
    )!;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
      setter.call(select, "bottom-left");
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(useKvmStore.getState().settings.indicatorHudCorner).toBe("bottom-left");
  });

  it("large-file limits keep their keys", async () => {
    await mount();
    await openSection("Advanced");
    const field = (label: string) => {
      const l = Array.from(el.querySelectorAll("label")).find((x) => x.textContent === label)!;
      return el.querySelector<HTMLInputElement>(`[id="${l.htmlFor}"]`)!;
    };
    await setInput(field("Large files: save to Downloads above"), "512");
    await setInput(field("Large files: ask before copying above"), "250");
    const s = useKvmStore.getState().settings;
    expect(s.continuityStagingLimitMb).toBe(512);
    expect(s.continuityClipPrefetchLimitMb).toBe(250);
  });

  it("dwell and deadzone are read-only in Settings and link to Computers", async () => {
    await mount();
    await openSection("Shared");
    expect(el.querySelector('input[type="range"]')).toBeNull();
    const out = Array.from(el.querySelectorAll("output")).map((o) => o.textContent);
    const s = useKvmStore.getState().settings;
    expect(out).toContain(`${s.edgeResistanceMs} ms`);
    expect(out).toContain(`${s.cornerDeadzonePx} px`);
    const before = JSON.stringify(s);
    const links = Array.from(el.querySelectorAll("button")).filter(
      (b) => b.textContent === "Edit in Computers",
    );
    expect(links).toHaveLength(2);
    await act(async () => links[0].click());
    expect(onOpenComputers).toHaveBeenCalledTimes(1);
    expect(JSON.stringify(useKvmStore.getState().settings)).toBe(before);
  });

  it("renders F-01..F-03 transfer folder controls, validates phone path, and persists changes", async () => {
    await mount();
    await openSection("Clipboard");

    expect(el.textContent).toContain("Save received files to");
    expect(el.textContent).toContain("C:\\Users\\Tama\\Downloads\\AnyLeap");
    expect(el.textContent).toContain("Ask where to save each time");
    expect(el.textContent).toContain("Save files sent to phone in");

    // F-01: Change... and Open and Reset
    mockOpenDialog.mockResolvedValueOnce("D:\\Incoming\\AnyLeap");
    const changeBtn = Array.from(el.querySelectorAll("button")).find(
      (b) => b.textContent === "Change...",
    )!;
    await act(async () => changeBtn.click());
    expect(useAppStore.getState().transferFolders.receiveDir).toBe("D:\\Incoming\\AnyLeap");
    expect(mockSetTransferFoldersConfig).toHaveBeenCalledWith(
      "D:\\Incoming\\AnyLeap",
      "/sdcard/Download",
    );

    const openBtn = Array.from(el.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Open"),
    )!;
    await act(async () => openBtn.click());
    expect(mockRevealItemInDir).toHaveBeenCalledWith("D:\\Incoming\\AnyLeap");

    // F-02: Ask where to save each time switch
    const askSwitch = sw("Ask where to save each time");
    expect(askSwitch.getAttribute("aria-checked")).toBe("false");
    expect(askSwitch.getAttribute("aria-describedby")).toBeTruthy();
    await act(async () => askSwitch.click());
    expect(useAppStore.getState().transferFolders.askWhereToSave).toBe(true);

    // F-03: Preset chips update phoneUploadDir
    const docsChip = Array.from(el.querySelectorAll("button")).find(
      (b) => b.textContent === "Documents",
    )!;
    await act(async () => docsChip.click());
    expect(useAppStore.getState().transferFolders.phoneUploadDir).toBe("/sdcard/Documents");
    expect(mockSetTransferFoldersConfig).toHaveBeenLastCalledWith(
      "D:\\Incoming\\AnyLeap",
      "/sdcard/Documents",
    );

    // F-03: Invalid path shows inline error and does not call backend sync
    mockSetTransferFoldersConfig.mockClear();
    const phoneLabel = Array.from(el.querySelectorAll("label")).find(
      (l) => l.textContent === "Save files sent to phone in",
    )!;
    const phoneInput = el.querySelector<HTMLInputElement>(`[id="${phoneLabel.htmlFor}"]`)!;
    await setInput(phoneInput, "/data/local/tmp");
    expect(el.querySelector('[role="alert"]')?.textContent).toContain(
      "Enter a folder inside /sdcard or /storage/emulated/0",
    );
    expect(mockSetTransferFoldersConfig).not.toHaveBeenCalled();

    // T-04 helper no longer mentions AnyLeap_Transfers
    expect(entry("T-04").helper).not.toContain("AnyLeap_Transfers");
  });
});
