import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "@/store/useAppStore";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
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
const mockGetPhoneToggles = vi.fn((..._args: unknown[]) =>
  Promise.resolve({ wifi: true, bluetooth: false, dnd: false }),
);
const mockSetPhoneToggle = vi.fn((..._args: unknown[]) => Promise.resolve());

vi.mock("@/services/phoneHubService", () => ({
  phoneHubWatch: (...args: unknown[]) => mockPhoneHubWatch(...args),
  phoneHubUnwatch: (...args: unknown[]) => mockPhoneHubUnwatch(...args),
  phoneGetOverview: (...args: unknown[]) => mockPhoneGetOverview(...args),
  phoneGetConversations: (...args: unknown[]) => mockPhoneGetConversations(...args),
  phoneAudioStart: vi.fn(() => Promise.resolve()),
  phoneAudioStop: vi.fn(() => Promise.resolve()),
  onPhoneOverview: vi.fn(() => Promise.resolve(() => {})),
  onPhoneAudioState: vi.fn(() => Promise.resolve(() => {})),
  onPhoneMessagesUpdated: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("@/lib/tauri", () => ({
  getPhoneToggles: (...args: unknown[]) => mockGetPhoneToggles(...args),
  setPhoneToggle: (...args: unknown[]) => mockSetPhoneToggle(...args),
  startMirror: vi.fn(() => Promise.resolve()),
  stopMirror: vi.fn(() => Promise.resolve()),
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
    mockGetPhoneToggles.mockClear();
    mockSetPhoneToggle.mockClear();
    usePhoneHubStore.getState().reset();
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
});
