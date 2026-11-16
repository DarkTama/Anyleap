import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StudioDeck } from "../StudioDeck";
import { useAppStore } from "@/store/useAppStore";
import { useActivityStore } from "@/stores/activityStore";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import type { PhoneOverview } from "@/types/phone";

let overviewEventCallback: ((event: { payload: { serial: string; overview: PhoneOverview } }) => void) | null = null;

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (eventName: string, cb: (event: any) => void) => {
    if (eventName === "phone-overview") {
      overviewEventCallback = cb;
    }
    return () => {
      if (eventName === "phone-overview") overviewEventCallback = null;
    };
  }),
}));

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(async () => false),
  requestPermission: vi.fn(async () => "denied"),
  sendNotification: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  disconnectDevice: vi.fn(async () => {}),
  startMirror: vi.fn(async () => ({})),
  stopMirror: vi.fn(async () => {}),
  getBatteryInfo: vi.fn(async () => ({ level: 44, charging: false })),
  getDeviceDetails: vi.fn(async () => ({
    serial: "192.168.1.24:5555",
    model: "Pixel 8",
    manufacturer: "Google",
    androidVersion: "14",
    connection: "wifi",
  })),
  getSystemResolution: vi.fn(async () => [1920, 1080]),
  resetDeviceDisplay: vi.fn(async () => {}),
  openUrlOnDevice: vi.fn(async () => {}),
  placeCall: vi.fn(async () => ""),
}));

describe("StudioDeck (SPEC-27c Devices tab)", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    overviewEventCallback = null;
    usePhoneHubStore.getState().reset();
    useActivityStore.setState({ activities: [], loaded: true });
    useAppStore.setState({
      devices: [
        {
          serial: "192.168.1.24:5555",
          state: "device",
          model: "Pixel 8",
          product: "shiba",
          hw_serial: "HW-PIXEL",
        },
      ],
      savedDevices: [],
      sessions: [],
      batteryMap: { "192.168.1.24:5555": { level: 44, charging: false } },
      nicknames: {},
      selectedSerial: "192.168.1.24:5555",
      deviceDetailsCache: {},
      repairNeeded: {},
    });
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it("renders battery chip from fallback batteryMap and updates from phone-overview event", async () => {
    await act(async () => {
      root.render(<StudioDeck onOpenPair={vi.fn()} />);
    });

    expect(container.textContent).toContain("44%");
    expect(container.textContent).toContain("Wireless");

    await act(async () => {
      overviewEventCallback?.({
        payload: {
          serial: "192.168.1.24:5555",
          overview: {
            model: "Pixel 8",
            brand: "Google",
            androidVersion: "15",
            securityPatch: null,
            battery: { level: 89, charging: true, tempC: 30, health: "good" },
            storage: null,
            ram: null,
            wifi: null,
            cellular: null,
            screen: "on",
            uptimeSecs: 600,
            updatedAtMs: 1700000000000,
          },
        },
      });
    });

    expect(container.textContent).toContain("89%");
    expect(container.textContent).toContain("Android 15");
    expect(usePhoneHubStore.getState().overviewsBySerial["192.168.1.24:5555"]?.battery?.level).toBe(89);
  });

  it("navigates to Phone -> Overview when clicking View phone details", async () => {
    const onViewPhoneDetails = vi.fn();
    await act(async () => {
      root.render(<StudioDeck onViewPhoneDetails={onViewPhoneDetails} />);
    });

    const btn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("View phone details"),
    );
    expect(btn).toBeDefined();

    act(() => {
      btn?.click();
    });
    expect(onViewPhoneDetails).toHaveBeenCalledWith("192.168.1.24:5555");
  });

  it("shows Activity as a tab with red failed count and switches tabpanel", async () => {
    useActivityStore.setState({
      activities: [
        {
          id: "1",
          kind: "camera",
          title: "Failed to start camera",
          detail: "Camera busy",
          at: Date.now() - 60_000,
        },
        {
          id: "2",
          kind: "device",
          title: "Connected Pixel 8",
          detail: "192.168.1.24:5555",
          at: Date.now() - 120_000,
        },
      ],
      loaded: true,
    });

    await act(async () => {
      root.render(<StudioDeck />);
    });

    const activityTab = Array.from(container.querySelectorAll('[role="tab"]')).find((t) =>
      t.textContent?.includes("Activity"),
    ) as HTMLElement | undefined;
    expect(activityTab).toBeDefined();
    expect(activityTab?.textContent).toContain("2");
    expect(activityTab?.textContent).toContain("1 failed");

    act(() => {
      activityTab?.click();
    });

    expect(container.textContent).toContain("Failed to start camera");
    expect(container.textContent).toContain("Camera busy");
    expect(container.textContent).toContain("Connected Pixel 8");
  });
});
