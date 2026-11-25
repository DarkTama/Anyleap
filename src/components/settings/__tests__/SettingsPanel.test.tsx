import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/persist", async (orig) => ({
  ...(await orig<typeof import("@/lib/persist")>()),
  saveModeDefaults: vi.fn(async () => {}),
  saveModeDefaultsNotice: vi.fn(async () => {}),
  saveAppPrefs: vi.fn(async () => {}),
  saveTransferFolders: vi.fn(async () => {}),
  saveNotificationStorage: vi.fn(async () => {}),
}));
const mockPhoneSetNotificationStorageConfig = vi.fn(async (..._args: unknown[]) => ({
  historyEnabled: false,
  vaultError: null,
  savedHwSerials: [],
}));
const mockPhoneDeleteAppNotificationData = vi.fn(async (..._args: unknown[]) => ({
  deletedNotifications: 1,
  deletedTransactions: 0,
  deletedLiabilityEvents: 0,
}));
const mockPhoneDeleteAllNotificationData = vi.fn(async () => ({
  deletedNotifications: 2,
  deletedTransactions: 0,
  deletedLiabilityEvents: 0,
}));
const mockPhoneListApps = vi.fn(
  async () => [] as Array<{ package: string; label: string; system: boolean }>,
);
const mockPhoneGetSavedNotificationPackages = vi.fn(async () => [] as string[]);
const mockPhoneHubSetSettings = vi.fn(async (..._args: unknown[]) => {});
vi.mock("@/services/phoneHubService", () => ({
  phoneListApps: () => mockPhoneListApps(),
  phoneGetSavedNotificationPackages: () => mockPhoneGetSavedNotificationPackages(),
  phoneHubSetSettings: (...args: unknown[]) => mockPhoneHubSetSettings(...args),
  phoneSetNotificationStorageConfig: (...args: unknown[]) =>
    mockPhoneSetNotificationStorageConfig(...args),
  phoneDeleteAppNotificationData: (...args: unknown[]) =>
    mockPhoneDeleteAppNotificationData(...args),
  phoneDeleteAllNotificationData: () => mockPhoneDeleteAllNotificationData(),
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
import { DEFAULT_NOTIFICATION_STORAGE } from "@/lib/notificationStorage";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
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
    mockPhoneListApps.mockResolvedValue([]);
    mockPhoneGetSavedNotificationPackages.mockResolvedValue([]);
    localStorage.clear();
    useAppStore.getState().setDevices([]);
    useKvmStore.getState().reset();
    usePhoneHubStore.getState().reset();
    useAppStore.getState().setNotificationStorage({
      ...DEFAULT_NOTIFICATION_STORAGE,
      largeIncomeThresholdByCurrency: {},
      perApp: {},
    });
    useAppStore.getState().setVaultError(null);
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
    mockPhoneListApps.mockResolvedValue([]);
    mockPhoneGetSavedNotificationPackages.mockResolvedValue([]);
    localStorage.clear();
    useAppStore.getState().setDevices([]);
    useKvmStore.getState().reset();
    usePhoneHubStore.getState().reset();
    useAppStore.getState().setNotificationStorage({
      ...DEFAULT_NOTIFICATION_STORAGE,
      largeIncomeThresholdByCurrency: {},
      perApp: {},
    });
    useAppStore.getState().setVaultError(null);
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

  it("renders N-01..N-08, supports Add money source, downgrade modal, and Delete all modal", async () => {
    await mount();

    const saveSwitch = sw("Save notification history & transactions on this PC");
    const bgSwitch = sw("Capture notifications while Phone tab is closed");
    expect(saveSwitch.getAttribute("aria-checked")).toBe("false");
    expect(bgSwitch.hasAttribute("disabled")).toBe(true);

    await act(async () => saveSwitch.click());
    expect(useAppStore.getState().notificationStorage.historyEnabled).toBe(true);
    expect(mockPhoneSetNotificationStorageConfig).toHaveBeenCalled();
    expect(sw("Capture notifications while Phone tab is closed").hasAttribute("disabled")).toBe(false);

    // Add an app row via the "Add an app that isn't listed" disclosure
    const disclosureBtn = Array.from(el.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Add an app that isn't listed"),
    )!;
    await act(async () => disclosureBtn.click());
    const addInput = el.querySelector<HTMLInputElement>(
      'input[placeholder*="com.example.bank"]',
    )!;
    await setInput(addInput, "com.example.shop");
    const addAppBtn = Array.from(el.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Add app",
    )!;
    await act(async () => addAppBtn.click());
    expect(useAppStore.getState().notificationStorage.perApp["com.example.shop"]?.retention).toBe(
      "history",
    );

    // Add money source with multi-type checkboxes
    const addSourceBtn = Array.from(el.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Add money source"),
    )!;
    await act(async () => addSourceBtn.click());
    const creditCardCheckbox = Array.from(el.querySelectorAll<HTMLLabelElement>("label"))
      .find((lbl) => lbl.textContent?.includes("Credit card"))
      ?.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    await act(async () => creditCardCheckbox.click());
    const saveSourceBtn = Array.from(el.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Save source",
    )!;
    await act(async () => saveSourceBtn.click());

    const shopPolicy = useAppStore.getState().notificationStorage.perApp["com.example.shop"];
    expect(shopPolicy?.moneySources).toHaveLength(1);
    expect(shopPolicy?.moneySources[0].ruleTemplate).toBe("generic_credit_card");
    expect(shopPolicy?.moneySources[0].label).toBe("com.example.shop Credit card");
    expect(shopPolicy?.sensitive).toBe(true);

    // Downgrade to "none" prompts confirmation with checked-by-default delete checkbox
    const retSelect = el.querySelector<HTMLSelectElement>('select[id="ret-com.example.shop"]')!;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
      setter.call(retSelect, "none");
      retSelect.dispatchEvent(new Event("change", { bubbles: true }));
    });
    const dialog = el.querySelector('[role="dialog"]')!;
    expect(dialog.textContent).toContain("Also delete existing saved data for com.example.shop?");
    const confirmCheckbox = dialog.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(confirmCheckbox.checked).toBe(true);
    const confirmBtn = Array.from(dialog.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Confirm",
    )!;
    await act(async () => confirmBtn.click());
    expect(mockPhoneDeleteAppNotificationData).toHaveBeenCalledWith(
      "com.example.shop",
      undefined,
      false,
    );
    expect(useAppStore.getState().notificationStorage.perApp["com.example.shop"]?.retention).toBe(
      "none",
    );

    // Delete all modal requires confirmation before calling backend
    const deleteAllBtn = Array.from(el.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Delete all saved data"),
    )!;
    await act(async () => deleteAllBtn.click());
    expect(mockPhoneDeleteAllNotificationData).not.toHaveBeenCalled();
    const deleteDialog = el.querySelector('[role="dialog"]')!;
    const confirmDeleteAllBtn = Array.from(deleteDialog.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Delete everything",
    )!;
    await act(async () => confirmDeleteAllBtn.click());
    expect(mockPhoneDeleteAllNotificationData).toHaveBeenCalledTimes(1);
  });

  it("offers Delete & reinitialize when vault decryption fails", async () => {
    useAppStore
      .getState()
      .setVaultError("VAULT_DECRYPT_FAILED: Unable to decrypt local vault");
    await mount();
    expect(el.textContent).toContain("Unable to decrypt local vault for this Windows user");
    const resetBtn = Array.from(el.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Delete & reinitialize"),
    )!;
    await act(async () => resetBtn.click());
    expect(mockPhoneDeleteAllNotificationData).toHaveBeenCalledTimes(1);
  });

  it("filters apps by label/package with Esc to clear and Enter never creates rows", async () => {
    usePhoneHubStore.getState().applyNotifications("DEV1", [
      {
        key: "k1",
        package: "com.example.wallet",
        appLabel: "Sample Wallet",
        title: "Notice",
        text: "Paid",
        postedUnix: 1000,
      },
      {
        key: "k2",
        package: "com.example.notes",
        appLabel: "Quick Notes",
        title: "Sync",
        text: "Done",
        postedUnix: 999,
      },
    ]);
    await mount();

    const searchInput = el.querySelector<HTMLInputElement>('input[placeholder="Search apps"]')!;
    expect(searchInput).toBeTruthy();

    // Typing a query and pressing Enter never creates a row
    await setInput(searchInput, "mybank");
    await key(searchInput, "Enter");
    expect(useAppStore.getState().notificationStorage.perApp["mybank"]).toBeUndefined();
    expect(el.textContent).toContain("No apps match");

    // Esc clears search
    await key(searchInput, "Escape");
    expect(searchInput.value).toBe("");

    // Case-insensitive match by label or package auto-shows matching Other apps
    await setInput(searchInput, "WALLET");
    expect(el.querySelector('[data-app-policy-row="com.example.wallet"]')).toBeTruthy();
    expect(el.querySelector('[data-app-policy-row="com.example.notes"]')).toBeNull();

    await setInput(searchInput, "example.notes");
    expect(el.querySelector('[data-app-policy-row="com.example.notes"]')).toBeTruthy();
    expect(el.querySelector('[data-app-policy-row="com.example.wallet"]')).toBeNull();
  });

  it("validates package names and rejects duplicates in Add an app that isn't listed", async () => {
    usePhoneHubStore.getState().applyNotifications("DEV1", [
      {
        key: "k1",
        package: "com.example.listed",
        appLabel: "Listed App",
        title: "Hi",
        text: "Body",
        postedUnix: 1000,
      },
    ]);
    await mount();

    const disclosureBtn = Array.from(el.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Add an app that isn't listed"),
    )!;
    await act(async () => disclosureBtn.click());

    const pkgInput = el.querySelector<HTMLInputElement>('input[placeholder*="com.example.bank"]')!;
    const addBtn = Array.from(el.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Add app",
    )!;

    // Invalid package format shows inline error
    await setInput(pkgInput, "notapackage");
    await act(async () => addBtn.click());
    expect(el.querySelector('[role="alert"]')?.textContent).toContain(
      "Enter a valid Android package ID",
    );
    expect(useAppStore.getState().notificationStorage.perApp["notapackage"]).toBeUndefined();

    // Duplicate package already in the list is rejected
    await setInput(pkgInput, "com.example.listed");
    await act(async () => addBtn.click());
    expect(el.querySelector('[role="alert"]')?.textContent).toContain("already in your app list");
  });

  it("supports Remove from list with checked-by-default delete confirmation for manual rows", async () => {
    useAppStore.getState().setNotificationStorage({
      ...DEFAULT_NOTIFICATION_STORAGE,
      historyEnabled: true,
      largeIncomeThresholdByCurrency: {},
      perApp: {
        "com.example.manual": {
          retention: "history",
          retentionDays: 30,
          moneySources: [],
          sensitive: true,
        },
        "com.example.manual2": {
          retention: "history",
          retentionDays: 7,
          moneySources: [],
          sensitive: false,
        },
      },
    });
    await mount();

    const row1 = el.querySelector('[data-app-policy-row="com.example.manual"]')!;
    const removeBtn1 = Array.from(row1.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Remove from list"),
    )!;
    expect(removeBtn1).toBeTruthy();
    await act(async () => removeBtn1.click());

    const dialog1 = el.querySelector('[role="dialog"]')!;
    expect(dialog1.textContent).toContain("Remove com.example.manual from list?");
    const checkbox1 = dialog1.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(checkbox1.checked).toBe(true);
    const confirm1 = Array.from(dialog1.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Remove",
    )!;
    await act(async () => confirm1.click());

    expect(useAppStore.getState().notificationStorage.perApp["com.example.manual"]).toBeUndefined();
    expect(mockPhoneDeleteAppNotificationData).toHaveBeenCalledWith(
      "com.example.manual",
      undefined,
      true,
    );

    // Unchecking delete skips vault data deletion
    mockPhoneDeleteAppNotificationData.mockClear();
    const row2 = el.querySelector('[data-app-policy-row="com.example.manual2"]')!;
    const removeBtn2 = Array.from(row2.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Remove from list"),
    )!;
    await act(async () => removeBtn2.click());
    const dialog2 = el.querySelector('[role="dialog"]')!;
    const checkbox2 = dialog2.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    await act(async () => checkbox2.click());
    expect(checkbox2.checked).toBe(false);
    const confirm2 = Array.from(dialog2.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Remove",
    )!;
    await act(async () => confirm2.click());
    expect(useAppStore.getState().notificationStorage.perApp["com.example.manual2"]).toBeUndefined();
    expect(mockPhoneDeleteAppNotificationData).not.toHaveBeenCalled();
  });

  it("migrates invalid perApp entries on load: drops empty ones and keeps ones with saved data", async () => {
    mockPhoneGetSavedNotificationPackages.mockResolvedValue(["saved_bad_pkg"]);
    useAppStore.getState().setNotificationStorage({
      ...DEFAULT_NOTIFICATION_STORAGE,
      historyEnabled: true,
      largeIncomeThresholdByCurrency: {},
      perApp: {
        empty_bad_pkg: {
          retention: "history",
          retentionDays: 30,
          moneySources: [],
          sensitive: false,
        },
        saved_bad_pkg: {
          retention: "history",
          retentionDays: 30,
          moneySources: [],
          sensitive: false,
        },
      },
    });
    await mount();

    // Empty invalid entry was dropped
    expect(useAppStore.getState().notificationStorage.perApp["empty_bad_pkg"]).toBeUndefined();
    expect(el.querySelector('[data-app-policy-row="empty_bad_pkg"]')).toBeNull();

    // Entry with saved data is kept, flagged "Invalid package", and has Remove from list
    expect(useAppStore.getState().notificationStorage.perApp["saved_bad_pkg"]).toBeDefined();
    const keptRow = el.querySelector('[data-app-policy-row="saved_bad_pkg"]')!;
    expect(keptRow).toBeTruthy();
    expect(keptRow.textContent).toContain("Invalid package");
    expect(keptRow.textContent).toContain("Remove from list");
  });

  it("orders configured apps first, collapses unconfigured under Other apps (N), and hides Phase jargon", async () => {
    useAppStore.getState().setNotificationStorage({
      ...DEFAULT_NOTIFICATION_STORAGE,
      historyEnabled: true,
      largeIncomeThresholdByCurrency: {},
      perApp: {
        "com.example.configured": {
          retention: "transactions",
          retentionDays: 30,
          moneySources: [],
          sensitive: true,
        },
      },
    });
    usePhoneHubStore.getState().applyNotifications("DEV1", [
      {
        key: "k1",
        package: "com.example.other1",
        appLabel: "Alpha Other",
        title: "Ping",
        text: "Msg",
        postedUnix: 1000,
      },
      {
        key: "k2",
        package: "com.example.other2",
        appLabel: "Beta Other",
        title: "Ping",
        text: "Msg",
        postedUnix: 999,
      },
    ]);
    await mount();

    // No internal Phase jargon in the General section
    expect(el.textContent).not.toContain("Phase 3");
    expect(el.textContent).toContain("Money tracking is coming in a later update");

    // Configured row appears first with expanded Money sources (0)
    const configuredSection = el.querySelector('[data-app-section="configured"]')!;
    const otherSection = el.querySelector('[data-app-section="other"]')!;
    expect(
      configuredSection.compareDocumentPosition(otherSection) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    const configuredRow = configuredSection.querySelector(
      '[data-app-policy-row="com.example.configured"]',
    )!;
    expect(configuredRow.textContent).toContain("Money sources (0)");
    expect(configuredRow.textContent).toContain("Delete saved data");

    // Unconfigured apps are collapsed under "Other apps (2)"
    const otherToggle = Array.from(otherSection.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Other apps (2)"),
    )!;
    expect(otherToggle.getAttribute("aria-expanded")).toBe("false");
    expect(el.querySelector('[data-app-policy-row="com.example.other1"]')).toBeNull();

    // Expanding "Other apps (2)" shows compact rows with small Add money source and no Money sources (0)
    await act(async () => otherToggle.click());
    expect(otherToggle.getAttribute("aria-expanded")).toBe("true");
    const otherRow = el.querySelector('[data-app-policy-row="com.example.other1"]')!;
    expect(otherRow.textContent).toContain("Add money source");
    expect(otherRow.textContent).not.toContain("Money sources (0)");
  });

  it("hides Custom, renders user-friendly template labels, supports multi-type save, and handles Don't keep policy warning", async () => {
    usePhoneHubStore.getState().applyNotifications("DEV1", [
      {
        key: "k1",
        package: "com.example.wallet",
        appLabel: "Sample Wallet",
        title: "Notice",
        text: "Paid",
        postedUnix: 1000,
      },
    ]);
    await mount();

    // Expand Other apps and open Add money source on Sample Wallet
    const otherToggle = Array.from(el.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Other apps (1)"),
    )!;
    await act(async () => otherToggle.click());

    const walletRow = el.querySelector('[data-app-policy-row="com.example.wallet"]')!;
    const addSourceBtn = Array.from(walletRow.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Add money source"),
    )!;
    await act(async () => addSourceBtn.click());

    const updatedWalletRow = el.querySelector('[data-app-policy-row="com.example.wallet"]')!;
    // "Custom" is not offered; helper text is shown
    expect(updatedWalletRow.textContent).not.toContain("Custom");
    expect(updatedWalletRow.textContent).toContain(
      "Many wallets also offer paylater — tick both if you use both.",
    );

    // Tick both E-wallet and Paylater and save
    const checkboxes = Array.from(updatedWalletRow.querySelectorAll<HTMLLabelElement>("label"));
    const ewalletCb = checkboxes
      .find((lbl) => lbl.textContent?.includes("E-wallet"))
      ?.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    const paylaterCb = checkboxes
      .find((lbl) => lbl.textContent?.includes("Paylater"))
      ?.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    await act(async () => ewalletCb.click());
    await act(async () => paylaterCb.click());

    const saveBtn = Array.from(updatedWalletRow.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Save source",
    )!;
    await act(async () => saveBtn.click());

    // Creates 2 sources with generic templates and "<App label> <Type>" labels
    const walletPolicy = useAppStore.getState().notificationStorage.perApp["com.example.wallet"];
    expect(walletPolicy?.moneySources).toHaveLength(2);
    expect(walletPolicy?.moneySources[0]).toMatchObject({
      type: "ewallet",
      ruleTemplate: "generic_ewallet",
      label: "Sample Wallet E-wallet",
      enabled: true,
    });
    expect(walletPolicy?.moneySources[1]).toMatchObject({
      type: "paylater",
      ruleTemplate: "generic_paylater",
      label: "Sample Wallet Paylater",
      enabled: true,
    });

    // User-friendly template labels are rendered instead of raw ids
    const configuredWalletRow = el.querySelector('[data-app-policy-row="com.example.wallet"]')!;
    expect(configuredWalletRow.textContent).toContain("Generic e-wallet");
    expect(configuredWalletRow.textContent).toContain("Generic paylater");
    expect(configuredWalletRow.textContent).not.toContain("generic_ewallet");
    expect(configuredWalletRow.textContent).not.toContain("generic_paylater");
    expect(configuredWalletRow.textContent).toContain("Sample Wallet E-wallet");
    expect(configuredWalletRow.textContent).toContain("Sample Wallet Paylater");

    // Warning does not appear while retention is "transactions"
    expect(configuredWalletRow.textContent).not.toContain(
      "Set this app to Transactions only (or Keep history) so Money can count it.",
    );

    // Downgrade to "Don't keep" -> warning appears
    const retSelect = configuredWalletRow.querySelector<HTMLSelectElement>(
      'select[id="ret-com.example.wallet"]',
    )!;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!;
      setter.call(retSelect, "none");
      retSelect.dispatchEvent(new Event("change", { bubbles: true }));
    });
    const confirmBtn = Array.from(el.querySelector('[role="dialog"]')!.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Confirm",
    )!;
    await act(async () => confirmBtn.click());

    const warnedRow = el.querySelector('[data-app-policy-row="com.example.wallet"]')!;
    expect(warnedRow.textContent).toContain(
      "Set this app to Transactions only (or Keep history) so Money can count it.",
    );

    // One-click "Use Transactions only" switches retention and dismisses warning
    const fixBtn = Array.from(warnedRow.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Use Transactions only",
    )!;
    await act(async () => fixBtn.click());

    expect(useAppStore.getState().notificationStorage.perApp["com.example.wallet"]?.retention).toBe(
      "transactions",
    );
    const fixedRow = el.querySelector('[data-app-policy-row="com.example.wallet"]')!;
    expect(fixedRow.textContent).not.toContain(
      "Set this app to Transactions only (or Keep history) so Money can count it.",
    );

    // If all sources are disabled while retention is "none", warning stays hidden
    await act(async () => {
      useAppStore.getState().setNotificationStorage({
        ...useAppStore.getState().notificationStorage,
        perApp: {
          "com.example.wallet": {
            retention: "none",
            retentionDays: 30,
            sensitive: true,
            moneySources: [
              {
                id: "src_ewallet_1",
                type: "ewallet",
                ruleTemplate: "generic_ewallet",
                enabled: false,
                label: "Sample Wallet E-wallet",
              },
            ],
          },
        },
      });
    });
    const disabledSourceRow = el.querySelector('[data-app-policy-row="com.example.wallet"]')!;
    expect(disabledSourceRow.textContent).not.toContain(
      "Set this app to Transactions only (or Keep history) so Money can count it.",
    );
  });

  it("navigates to General and scrolls to Phone notifications & Money when focusTarget is provided", async () => {
    await mount();
    // Switch to another section first
    const railBtn = Array.from(el.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Phone mirroring"),
    )!;
    await act(async () => railBtn.click());
    expect(el.querySelector("#settings-phone-notifications-money")).toBeNull();

    const scrollSpy = vi.fn();
    const origScroll = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = scrollSpy;
    try {
      await act(async () => {
        root.render(
          <SettingsPanel
            onOpenComputers={onOpenComputers}
            focusTarget={{
              section: "general",
              anchorId: "settings-phone-notifications-money",
              nonce: 1,
            }}
          />,
        );
      });
      const panel = el.querySelector("#settings-phone-notifications-money")!;
      expect(panel).toBeTruthy();
      expect(scrollSpy).toHaveBeenCalled();
    } finally {
      HTMLElement.prototype.scrollIntoView = origScroll;
    }
  });
});
