import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "@/store/useAppStore";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import { DEFAULT_TRANSFER_FOLDERS } from "@/lib/transferFolders";
import { DEFAULT_NOTIFICATION_STORAGE } from "@/lib/notificationStorage";
import type { DeviceEntry } from "@/lib/types";
import type { PhoneOverview } from "@/types/phone";

const mockPhoneHubWatch = vi.fn((..._args: unknown[]) => Promise.resolve());
const mockPhoneHubUnwatch = vi.fn((..._args: unknown[]) => Promise.resolve());
const mockPhoneGetOverview = vi.fn<(...args: unknown[]) => Promise<PhoneOverview>>((..._args: unknown[]) =>
  Promise.resolve({
    model: null,
    brand: null,
    androidVersion: null,
    securityPatch: null,
    battery: null,
    storage: null,
    ram: null,
    wifi: null,
    cellular: null,
    screen: null,
    uptimeSecs: null,
    updatedAtMs: 1000,
  }),
);
const mockPhoneGetConversations = vi.fn((..._args: unknown[]) => Promise.resolve([]));
const mockPhoneGetSavedNotificationDevices = vi.fn<() => Promise<string[]>>(() =>
  Promise.resolve([]),
);
const mockPhoneGetNotificationHistory = vi.fn<(...args: unknown[]) => Promise<unknown[]>>(() =>
  Promise.resolve([]),
);
const mockGetPhoneToggles = vi.fn((..._args: unknown[]) =>
  Promise.resolve({ wifi: true, bluetooth: false, dnd: false }),
);
const mockSetPhoneToggle = vi.fn((..._args: unknown[]) => Promise.resolve());
const mockPushToDevice = vi.fn((..._args: unknown[]) => Promise.resolve());
const mockListDeviceDir = vi.fn<(...args: unknown[]) => Promise<DeviceEntry[]>>(() =>
  Promise.resolve([]),
);

type DragEventPayload =
  | { type: "enter"; paths: string[] }
  | { type: "over" }
  | { type: "leave" }
  | { type: "drop"; paths: string[] };

let dragListeners: Array<(e: { payload: DragEventPayload }) => void> = [];
const mockOnDragDropEvent = vi.fn(
  async (cb: (e: { payload: DragEventPayload }) => void) => {
    dragListeners.push(cb);
    return () => {
      dragListeners = dragListeners.filter((x) => x !== cb);
    };
  },
);

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: mockOnDragDropEvent,
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => null),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/api/path", () => ({
  downloadDir: vi.fn(async () => "C:\\Downloads"),
}));

vi.mock("@/services/phoneHubService", () => ({
  phoneHubWatch: (...args: unknown[]) => mockPhoneHubWatch(...args),
  phoneHubUnwatch: (...args: unknown[]) => mockPhoneHubUnwatch(...args),
  phoneGetOverview: (...args: unknown[]) => mockPhoneGetOverview(...args),
  phoneGetConversations: (...args: unknown[]) => mockPhoneGetConversations(...args),
  phoneGetSavedNotificationDevices: () => mockPhoneGetSavedNotificationDevices(),
  phoneGetNotificationHistory: (...args: unknown[]) =>
    mockPhoneGetNotificationHistory(...args),
  phoneSetNotificationHidden: vi.fn(() => Promise.resolve()),
  phoneDeleteAllNotificationData: vi.fn(() => Promise.resolve({})),
  phoneOpenApp: vi.fn(() => Promise.resolve({})),
  phoneAudioStart: vi.fn(() => Promise.resolve()),
  phoneAudioStop: vi.fn(() => Promise.resolve()),
  onPhoneOverview: vi.fn(() => Promise.resolve(() => {})),
  onPhoneAudioState: vi.fn(() => Promise.resolve(() => {})),
  onPhoneMessagesUpdated: vi.fn(() => Promise.resolve(() => {})),
  onPhoneNotifications: vi.fn(() => Promise.resolve(() => {})),
  phoneGetTransactions: vi.fn(() =>
    Promise.resolve({
      transactions: [],
      liabilityEvents: [],
      subAccountNicknames: {},
      currencies: [],
      ruleWarnings: [],
    }),
  ),
  phoneResolveLiabilityEvent: vi.fn(() => Promise.resolve({})),
  phoneApplyTransactionCorrection: vi.fn(() => Promise.resolve({})),
}));

vi.mock("@/lib/tauri", () => ({
  getPhoneToggles: (...args: unknown[]) => mockGetPhoneToggles(...args),
  setPhoneToggle: (...args: unknown[]) => mockSetPhoneToggle(...args),
  startMirror: vi.fn(() => Promise.resolve()),
  stopMirror: vi.fn(() => Promise.resolve()),
  pushToDevice: (...args: unknown[]) => mockPushToDevice(...args),
  listDeviceDir: (...args: unknown[]) => mockListDeviceDir(...args),
  pullDeviceFile: vi.fn(() => Promise.resolve({ savedPath: "C:\\Downloads\\AnyLeap\\f.txt", fallbackFrom: null })),
  deleteDevicePath: vi.fn(() => Promise.resolve()),
}));

import { PhoneView } from "../PhoneView";
import { OverviewView } from "../OverviewView";

describe("PhoneView & OverviewView (SPEC-27b Task B2)", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    mockPhoneHubWatch.mockClear();
    mockPhoneHubUnwatch.mockClear();
    mockPhoneGetOverview.mockClear();
    mockPhoneGetConversations.mockClear();
    mockPhoneGetSavedNotificationDevices.mockReset();
    mockPhoneGetSavedNotificationDevices.mockResolvedValue([]);
    mockPhoneGetNotificationHistory.mockReset();
    mockPhoneGetNotificationHistory.mockResolvedValue([]);
    mockGetPhoneToggles.mockClear();
    mockSetPhoneToggle.mockClear();
    mockPushToDevice.mockClear();
    mockListDeviceDir.mockReset();
    mockListDeviceDir.mockResolvedValue([]);
    dragListeners = [];
    mockOnDragDropEvent.mockClear();
    usePhoneHubStore.getState().reset();
    useAppStore.getState().setNotificationStorage({
      ...DEFAULT_NOTIFICATION_STORAGE,
      largeIncomeThresholdByCurrency: {},
      perApp: {},
    });
    useAppStore.getState().setSavedDevices([]);
    useAppStore.getState().setTransferFolders({ ...DEFAULT_TRANSFER_FOLDERS });
    useAppStore.setState({
      devices: [
        {
          serial: "192.168.1.50:5555",
          state: "device",
          model: "Pixel 8",
          product: "shiba",
        },
      ],
      sessions: [],
    });
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
  });

  it("calls phone_hub_watch on mount and phone_hub_unwatch on unmount", async () => {
    usePhoneHubStore.getState().applyConversations("192.168.1.50:5555", [
      {
        package: "app.example.chat",
        appLabel: "Chat",
        key: "k1",
        title: "Alex",
        isGroup: false,
        messages: [{ sender: "Alex", text: "Hey", timestampMs: 100 }],
        unread: 3,
      },
    ]);

    await act(async () => {
      root.render(
        <PhoneView
          subTab="overview"
          onSubTabChange={() => {}}
          selectedSerial="192.168.1.50:5555"
          onSelectSerial={() => {}}
          notifications={[]}
          notificationsLoading={false}
          notificationsError={null}
          onRefreshNotifications={() => {}}
        />,
      );
    });

    expect(mockPhoneHubWatch).toHaveBeenCalledWith("192.168.1.50:5555", [
      "overview",
      "notifications",
    ]);
    expect(mockPhoneGetOverview).toHaveBeenCalledWith("192.168.1.50:5555", true);
    expect(container.textContent).toContain("Overview");
    expect(container.textContent).toContain("Messages");
    expect(container.textContent).toContain("3");
    expect(container.textContent).toContain("Wireless");

    act(() => {
      root.unmount();
    });

    expect(mockPhoneHubUnwatch).toHaveBeenCalledWith("192.168.1.50:5555");
  });

  it("renders resolved overview data immediately on mount without waiting for poll tick", async () => {
    mockPhoneGetOverview.mockResolvedValueOnce({
      model: "Pixel 8",
      brand: "Google",
      androidVersion: "14",
      securityPatch: "2026-10-01",
      battery: { level: 82, charging: false, tempC: 30, health: "good" },
      storage: null,
      ram: null,
      wifi: null,
      cellular: null,
      screen: "on",
      uptimeSecs: 3600,
      updatedAtMs: Date.now(),
    });

    await act(async () => {
      root.render(
        <PhoneView
          subTab="overview"
          onSubTabChange={() => {}}
          selectedSerial="192.168.1.50:5555"
          onSelectSerial={() => {}}
          notifications={[]}
          notificationsLoading={false}
          notificationsError={null}
          onRefreshNotifications={() => {}}
        />,
      );
    });

    expect(mockPhoneGetOverview).toHaveBeenCalledWith("192.168.1.50:5555", true);
    expect(container.textContent).toContain("Pixel 8");
    expect(container.textContent).toContain("82%");
  });

  it("shows notification and muted app counts in Notifications page subtitle without duplicating toolbar text", async () => {
    usePhoneHubStore.getState().setMutedPackages(["app.example.games", "app.example.ads"]);

    await act(async () => {
      root.render(
        <PhoneView
          subTab="notifications"
          onSubTabChange={() => {}}
          selectedSerial="192.168.1.50:5555"
          onSelectSerial={() => {}}
          notifications={[
            {
              key: "k1",
              package: "app.example.chat",
              appLabel: "Chat",
              title: "Alex",
              text: "Hey",
              postedUnix: 100,
            },
          ]}
          notificationsLoading={false}
          notificationsError={null}
          onRefreshNotifications={() => {}}
        />,
      );
    });

    expect(container.textContent).toContain("1 notification · 2 apps muted");
    const matches = container.textContent?.match(/Read-only — replies need the phone/g) ?? [];
    expect(matches).toHaveLength(1);
  });

  it("renders em-dash for null leaves, hidden Wi-Fi name, and confirms wireless Wi-Fi off", async () => {
    usePhoneHubStore.getState().applyOverview("192.168.1.50:5555", {
      model: "Work phone",
      brand: null,
      androidVersion: "12",
      securityPatch: null,
      battery: null,
      storage: null,
      ram: null,
      wifi: { ssid: null, signalLevel: 2 },
      cellular: null,
      screen: "off",
      uptimeSecs: null,
      updatedAtMs: 1000,
    });

    await act(async () => {
      root.render(<OverviewView serial="192.168.1.50:5555" isWireless={true} />);
    });

    expect(container.textContent).toContain("Battery details unavailable.");
    expect(container.textContent).toContain("Wi-Fi (name hidden)");
    expect(container.textContent).toContain("—");

    const wifiSwitch = container.querySelector<HTMLButtonElement>(
      'button[role="switch"][aria-label="Wi-Fi"]',
    );
    expect(wifiSwitch).not.toBeNull();
    expect(wifiSwitch?.disabled).toBe(false);

    await act(async () => {
      wifiSwitch?.click();
    });

    expect(container.textContent).toContain(
      "This phone is connected over Wi-Fi. Turning Wi-Fi off will disconnect it from AnyLeap.",
    );
    expect(mockSetPhoneToggle).not.toHaveBeenCalled();

    const confirmBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Turn off & disconnect"),
    );
    await act(async () => {
      confirmBtn?.click();
    });

    expect(mockSetPhoneToggle).toHaveBeenCalledWith(
      "192.168.1.50:5555",
      "wifi",
      false,
    );
  });

  it("handles OS drag-drop once in PhoneView: uploads to phoneUploadDir off Files and currentPath on Files", async () => {
    useAppStore.getState().setTransferFolders({
      ...DEFAULT_TRANSFER_FOLDERS,
      phoneUploadDir: "/sdcard/Documents",
    });
    mockListDeviceDir
      .mockResolvedValueOnce([
        {
          name: "Work",
          path: "/storage/emulated/0/Work",
          is_dir: true,
          size: 0,
          modified_unix: 100,
        },
      ])
      .mockResolvedValue([]);

    const renderWithTab = async (subTab: "overview" | "files") => {
      await act(async () => {
        root.render(
          <PhoneView
            subTab={subTab}
            onSubTabChange={() => {}}
            selectedSerial="192.168.1.50:5555"
            onSelectSerial={() => {}}
            notifications={[]}
            notificationsLoading={false}
            notificationsError={null}
            onRefreshNotifications={() => {}}
          />,
        );
      });
    };

    await renderWithTab("overview");
    expect(mockOnDragDropEvent).toHaveBeenCalledTimes(1);
    expect(dragListeners).toHaveLength(1);

    // Drag enter on Overview shows drop overlay naming phoneUploadDir
    await act(async () => {
      dragListeners[0]({ payload: { type: "enter", paths: ["C:\\a.txt"] } });
    });
    expect(container.textContent).toContain("Drop files to upload to phone");
    expect(container.textContent).toContain("Destination: /sdcard/Documents");

    // Drop uploads once to phoneUploadDir and shows completion toast
    await act(async () => {
      dragListeners[0]({ payload: { type: "drop", paths: ["C:\\a.txt", "C:\\b.jpg"] } });
    });
    expect(mockPushToDevice).toHaveBeenCalledTimes(1);
    expect(mockPushToDevice).toHaveBeenCalledWith(
      "192.168.1.50:5555",
      ["C:\\a.txt", "C:\\b.jpg"],
      "/sdcard/Documents",
    );
    expect(container.textContent).toContain("Uploaded 2 item(s) to /sdcard/Documents");

    // Switch to Files sub-tab and navigate into /storage/emulated/0/Work
    mockPushToDevice.mockClear();
    await renderWithTab("files");
    expect(mockOnDragDropEvent).toHaveBeenCalledTimes(1);

    const workCell = Array.from(container.querySelectorAll("td")).find((td) =>
      td.textContent?.includes("Work"),
    )!;
    await act(async () => {
      workCell.click();
    });

    await act(async () => {
      dragListeners[0]({ payload: { type: "over" } });
    });
    expect(container.textContent).toContain("Destination: /storage/emulated/0/Work");

    await act(async () => {
      dragListeners[0]({ payload: { type: "drop", paths: ["C:\\report.pdf"] } });
    });
    expect(mockPushToDevice).toHaveBeenCalledTimes(1);
    expect(mockPushToDevice).toHaveBeenCalledWith(
      "192.168.1.50:5555",
      ["C:\\report.pdf"],
      "/storage/emulated/0/Work",
    );
    expect(container.textContent).toContain("Uploaded 1 item(s) to /storage/emulated/0/Work");
  });

  it("shows saved offline notification history for a known device when persistence is enabled", async () => {
    const nowSec = Math.floor(Date.now() / 1000);
    useAppStore.setState({
      devices: [],
      savedDevices: [
        {
          id: "HW_OFFLINE_1",
          hw_serial: "HW_OFFLINE_1",
          label: "Pixel Saved",
          host: "192.168.1.99",
          port: 5555,
          lastSerial: "192.168.1.99:5555",
        },
      ],
      notificationStorage: {
        ...DEFAULT_NOTIFICATION_STORAGE,
        historyEnabled: true,
        largeIncomeThresholdByCurrency: {},
        perApp: {},
      },
    });
    mockPhoneGetNotificationHistory.mockResolvedValueOnce([
      {
        dedupeId: `HW_OFFLINE_1::k1::${nowSec - 30}`,
        hwSerial: "HW_OFFLINE_1",
        key: "k1",
        package: "com.example.chat",
        appLabel: "Example Chat",
        title: "Saved offline message",
        text: "Still encrypted locally",
        postedUnix: nowSec - 30,
        capturedUnix: nowSec - 25,
        hiddenLocal: false,
      },
    ]);

    await act(async () => {
      root.render(
        <PhoneView
          subTab="notifications"
          onSubTabChange={() => {}}
          selectedSerial={null}
          onSelectSerial={() => {}}
          notifications={[]}
          notificationsLoading={false}
          notificationsError={null}
          onRefreshNotifications={() => {}}
        />,
      );
    });

    expect(mockPhoneHubWatch).not.toHaveBeenCalled();
    expect(container.textContent).toContain("Offline — showing saved history");
    expect(container.textContent).toContain("Still encrypted locally");
  });

  it("renders Money tab below Notifications and wires Settings navigation from setup state", async () => {
    const onOpenSettings = vi.fn();
    await act(async () => {
      root.render(
        <PhoneView
          subTab="money"
          onSubTabChange={() => {}}
          selectedSerial="192.168.1.50:5555"
          onSelectSerial={() => {}}
          notifications={[]}
          notificationsLoading={false}
          notificationsError={null}
          onRefreshNotifications={() => {}}
          onOpenSettings={onOpenSettings}
        />,
      );
    });

    const navButtons = Array.from(container.querySelectorAll('nav[aria-label="Phone sections"] button'));
    const labels = navButtons.map((b) => b.textContent?.trim());
    expect(labels.indexOf("Money")).toBe(labels.indexOf("Notifications") + 1);

    const openSettingsBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Open Settings"),
    );
    expect(openSettingsBtn).toBeTruthy();
    await act(async () => {
      openSettingsBtn!.click();
    });
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });
});
