import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DeviceRack } from "../../DeviceRack";
import { useAppStore } from "@/store/useAppStore";

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(async () => false),
  requestPermission: vi.fn(async () => "denied"),
  sendNotification: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  disconnectDevice: vi.fn(async () => {}),
  startMirror: vi.fn(async () => ({})),
  stopMirror: vi.fn(async () => {}),
  getBatteryInfo: vi.fn(async () => ({ level: 82, charging: false })),
  openUrlOnDevice: vi.fn(async () => {}),
  placeCall: vi.fn(async () => ""),
}));

describe("DeviceRack", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    useAppStore.setState({
      devices: [],
      savedDevices: [],
      sessions: [],
      batteryMap: {},
      nicknames: {},
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

  it("renders empty state when no devices are present", () => {
    act(() => {
      root.render(
        <DeviceRack selectedSerial={null} onSelectDevice={vi.fn()} />,
      );
    });
    expect(container.textContent).toContain("No devices detected");
    expect(container.textContent).toContain("0");
  });

  it("renders live devices and selects on click", async () => {
    const onSelectDevice = vi.fn();
    useAppStore.setState({
      devices: [
        {
          serial: "USB-123",
          state: "device",
          model: "Aster A5",
          product: "aster",
          hw_serial: "HW-A5",
        },
      ],
      batteryMap: { "USB-123": { level: 65, charging: false } },
    });

    await act(async () => {
      root.render(
        <DeviceRack selectedSerial="USB-123" onSelectDevice={onSelectDevice} />,
      );
    });

    expect(container.textContent).toContain("Aster A5");
    expect(container.textContent).toContain("USB");
    expect(container.textContent).toContain("82%");

    const card = container.querySelector(".cursor-pointer") as HTMLElement | null;
    expect(card).not.toBeNull();
    act(() => {
      card?.click();
    });
    expect(onSelectDevice).toHaveBeenCalledWith("USB-123");
  });
});
