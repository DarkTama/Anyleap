import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("./tauri", () => ({
  listDevices: vi.fn(),
  discoverWireless: vi.fn(),
  reconnectWireless: vi.fn(),
}));

vi.mock("./settingsStore", () => ({
  replaceSavedDevices: vi.fn(async (d) => d),
  replaceNicknames: vi.fn(async (n) => n),
}));

import { useAppStore } from "../store/useAppStore";
import { adbErrorDeduper } from "./adbError";
import {
  DEVICE_SYNC_COOLDOWN_MS,
  reconnectSavedDevice,
  refreshAndReconnectDevices,
  resetDeviceSyncStateForTest,
  shouldReuseRecentDeviceSync,
  syncDevicesAndMigrate,
} from "./deviceSync";
import { discoverWireless, listDevices, reconnectWireless } from "./tauri";
import type { SavedDevice } from "./types";

describe("deviceSync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetDeviceSyncStateForTest();
    adbErrorDeduper.reset();
    useAppStore.setState({
      devices: [],
      savedDevices: [],
      nicknames: {},
      selectedSerial: null,
      repairNeeded: {},
      error: null,
    });
  });

  it("sequences listDevices before discoverWireless (never concurrent)", async () => {
    const events: string[] = [];
    let listDone = false;

    vi.mocked(listDevices).mockImplementation(async () => {
      events.push("list:start");
      await Promise.resolve();
      listDone = true;
      events.push("list:end");
      return [];
    });

    vi.mocked(discoverWireless).mockImplementation(async () => {
      events.push(`mdns:start(listDone=${listDone})`);
      return [];
    });

    await syncDevicesAndMigrate();

    expect(events).toEqual(["list:start", "list:end", "mdns:start(listDone=true)"]);
  });

  it("coalesces concurrent syncDevicesAndMigrate calls into one in-flight sync", async () => {
    let resolveList!: () => void;
    vi.mocked(listDevices).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveList = () => resolve([]);
        }),
    );
    vi.mocked(discoverWireless).mockResolvedValue([]);

    const p1 = syncDevicesAndMigrate();
    const p2 = syncDevicesAndMigrate();
    const p3 = syncDevicesAndMigrate();

    expect(listDevices).toHaveBeenCalledTimes(1);
    resolveList();
    await Promise.all([p1, p2, p3]);
    expect(discoverWireless).toHaveBeenCalledTimes(1);
  });

  it("coalesces concurrent refreshAndReconnectDevices calls", async () => {
    let resolveList!: () => void;
    vi.mocked(listDevices).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveList = () => resolve([]);
        }),
    );
    vi.mocked(discoverWireless).mockResolvedValue([]);

    const r1 = refreshAndReconnectDevices({ silentRepair: true });
    const r2 = refreshAndReconnectDevices({ silentRepair: true });

    expect(listDevices).toHaveBeenCalledTimes(1);
    resolveList();
    await Promise.all([r1, r2]);
    expect(listDevices).toHaveBeenCalledTimes(1);
  });

  it("deduplicates repeated ADB startup/reconnect errors surfaced to UI", async () => {
    const savedA: SavedDevice = {
      id: "hw-a",
      hw_serial: "hw-a",
      label: "Phone A",
      host: "192.168.1.10",
      port: 5555,
      lastSerial: "192.168.1.10:5555",
    };
    const savedB: SavedDevice = {
      id: "hw-b",
      hw_serial: "hw-b",
      label: "Phone B",
      host: "192.168.1.11",
      port: 5555,
      lastSerial: "192.168.1.11:5555",
    };

    const setErrorSpy = vi.spyOn(useAppStore.getState(), "setError");
    useAppStore.setState({ setError: setErrorSpy });

    vi.mocked(reconnectWireless).mockRejectedValue(
      new Error("Could not start ADB server on port 5037. Close any conflicting ADB tool using port 5037 and click Refresh."),
    );

    await Promise.all([
      reconnectSavedDevice(savedA, { silent: false, skipSync: true }),
      reconnectSavedDevice(savedB, { silent: false, skipSync: true }),
    ]);

    expect(setErrorSpy).toHaveBeenCalledTimes(1);
    expect(setErrorSpy.mock.calls[0][0]).toContain("Could not start ADB server on port 5037");
  });

  it("shouldReuseRecentDeviceSync enforces cooldown window unless forced", () => {
    expect(shouldReuseRecentDeviceSync(null, 5000)).toBe(false);
    expect(shouldReuseRecentDeviceSync(1000, 1000)).toBe(true);
    expect(shouldReuseRecentDeviceSync(1000, 1000 + DEVICE_SYNC_COOLDOWN_MS - 1)).toBe(true);
    expect(shouldReuseRecentDeviceSync(1000, 1000 + DEVICE_SYNC_COOLDOWN_MS)).toBe(false);
    expect(shouldReuseRecentDeviceSync(1000, 1500, DEVICE_SYNC_COOLDOWN_MS, true)).toBe(false);
  });

  it("debounces back-to-back syncDevicesAndMigrate and refreshAndReconnectDevices within cooldown unless forced", async () => {
    vi.mocked(listDevices).mockResolvedValue([]);
    vi.mocked(discoverWireless).mockResolvedValue([]);

    await syncDevicesAndMigrate();
    await syncDevicesAndMigrate();
    expect(listDevices).toHaveBeenCalledTimes(1);

    await syncDevicesAndMigrate(undefined, undefined, { force: true });
    expect(listDevices).toHaveBeenCalledTimes(2);

    await refreshAndReconnectDevices({ silentRepair: true });
    await refreshAndReconnectDevices({ silentRepair: true });
    expect(listDevices).toHaveBeenCalledTimes(2);

    await refreshAndReconnectDevices({ silentRepair: true, force: true });
    expect(listDevices).toHaveBeenCalledTimes(3);
  });
});
