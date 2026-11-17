import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  backoffDelayMs,
  createDeviceHealthMonitor,
  ERROR_RECONNECT_DEBOUNCE_MS,
  HEALTH_CHECK_INTERVAL_MS,
  parseDeviceGoneError,
  type HealthDeps,
} from "./deviceHealth";
import { buildRows } from "./deviceStatus";
import type { DeviceInfo, ReconnectResult, SavedDevice, SessionInfo } from "./types";

const saved: SavedDevice = {
  id: "HW1",
  hw_serial: "HW1",
  label: "Phone",
  host: "192.168.0.175",
  port: 37317,
  lastSerial: "192.168.0.175:37317",
  transports: { usb: null, wifi: "192.168.0.175:37317" },
};
const live: DeviceInfo = {
  serial: "192.168.0.175:37317",
  hw_serial: "HW1",
  state: "device",
  model: "TECNO",
  product: "p",
  transports: { usb: null, wifi: "192.168.0.175:37317" },
};
const ok: ReconnectResult = {
  status: "reconnected",
  serial: "192.168.0.175:44561",
  host: "192.168.0.175",
  port: 44561,
  hwSerial: "HW1",
  message: null,
} as ReconnectResult;
const bad = { ...ok, status: "needs_repair" } as ReconnectResult;

function setup(overrides: Partial<HealthDeps> = {}) {
  let devices: DeviceInfo[] = [live];
  let sessions: SessionInfo[] = [];
  let busy = false;
  const reconnect = vi.fn(async (_s: SavedDevice) => bad);
  const reconnecting: Array<[string, boolean]> = [];
  const deps: HealthDeps = {
    listDevices: vi.fn(async () => devices),
    reconnect,
    getState: () => ({ savedDevices: [saved], sessions }),
    setLiveDevices: vi.fn(),
    setReconnecting: (k, on) => reconnecting.push([k, on]),
    isBusy: () => busy,
    isHidden: () => false,
    now: () => Date.now(),
    ...overrides,
  };
  const mon = createDeviceHealthMonitor(deps);
  return {
    mon,
    deps,
    reconnect,
    reconnecting,
    setDevices: (d: DeviceInfo[]) => (devices = d),
    setSessions: (s: SessionInfo[]) => (sessions = s),
    setBusy: (b: boolean) => (busy = b),
  };
}

describe("deviceHealth", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
  });
  afterEach(() => vi.useRealTimers());

  it("polls every 15s", async () => {
    const t = setup();
    t.mon.start();
    expect(t.deps.listDevices).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(HEALTH_CHECK_INTERVAL_MS);
    expect(t.deps.listDevices).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(HEALTH_CHECK_INTERVAL_MS);
    expect(t.deps.listDevices).toHaveBeenCalledTimes(2);
    t.mon.stop();
    await vi.advanceTimersByTimeAsync(HEALTH_CHECK_INTERVAL_MS * 3);
    expect(t.deps.listDevices).toHaveBeenCalledTimes(2);
  });

  it("does not reconnect a present device, and publishes the live list", async () => {
    const t = setup();
    await t.mon.tick();
    expect(t.reconnect).not.toHaveBeenCalled();
    expect(t.deps.setLiveDevices).toHaveBeenCalled();
  });

  it("reconnects a saved device missing from the live list and flags Reconnecting", async () => {
    const t = setup();
    t.setDevices([]);
    t.reconnect.mockResolvedValueOnce(ok);
    await t.mon.tick();
    expect(t.reconnect).toHaveBeenCalledTimes(1);
    expect(t.reconnecting).toEqual([
      ["HW1", true],
      ["HW1", false],
    ]);
  });

  it("skips devices with an active session", async () => {
    const t = setup();
    t.setDevices([]);
    t.setSessions([{ id: "s", serial: "192.168.0.175:37317" } as SessionInfo]);
    await t.mon.tick();
    expect(t.reconnect).not.toHaveBeenCalled();
  });

  it("backs off 15s, 30s, 60s then caps at 60s", async () => {
    expect([1, 2, 3, 4, 9].map(backoffDelayMs)).toEqual([15000, 30000, 60000, 60000, 60000]);
    const t = setup();
    t.setDevices([]);
    await t.mon.tick(); // fail 1 -> wait 15s
    expect(t.reconnect).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(14_000);
    await t.mon.tick();
    expect(t.reconnect).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1_000);
    await t.mon.tick(); // fail 2 -> wait 30s
    expect(t.reconnect).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(29_000);
    await t.mon.tick();
    expect(t.reconnect).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1_000);
    await t.mon.tick(); // fail 3 -> wait 60s
    expect(t.reconnect).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(60_000);
    await t.mon.tick(); // fail 4 -> still 60s
    expect(t.reconnect).toHaveBeenCalledTimes(4);
    await vi.advanceTimersByTimeAsync(59_000);
    await t.mon.tick();
    expect(t.reconnect).toHaveBeenCalledTimes(4);
    await vi.advanceTimersByTimeAsync(1_000);
    await t.mon.tick();
    expect(t.reconnect).toHaveBeenCalledTimes(5);
  });

  it("does not overlap ticks", async () => {
    let release: (v: DeviceInfo[]) => void = () => {};
    const t = setup({
      listDevices: vi.fn(() => new Promise<DeviceInfo[]>((r) => (release = r))),
    });
    const first = t.mon.tick();
    await t.mon.tick();
    expect(t.deps.listDevices).toHaveBeenCalledTimes(1);
    release([live]);
    await first;
    const second = t.mon.tick();
    release([live]);
    await second;
    expect(t.deps.listDevices).toHaveBeenCalledTimes(2);
  });

  it("skips while a manual refresh/reconnect is running or window hidden", async () => {
    const t = setup();
    t.setBusy(true);
    await t.mon.tick();
    expect(t.deps.listDevices).not.toHaveBeenCalled();
    t.setBusy(false);
    const h = setup({ isHidden: () => true });
    await h.mon.tick();
    expect(h.deps.listDevices).not.toHaveBeenCalled();
  });

  it("does nothing without saved wireless devices", async () => {
    const t = setup({ getState: () => ({ savedDevices: [], sessions: [] }) });
    await t.mon.tick();
    expect(t.deps.listDevices).not.toHaveBeenCalled();
  });

  describe("error trigger", () => {
    const err = "adb.exe: device '192.168.0.175:37317' not found";

    it("parses gone errors", () => {
      expect(parseDeviceGoneError(err)).toEqual({ gone: true, serial: "192.168.0.175:37317" });
      expect(parseDeviceGoneError("error: device offline").gone).toBe(true);
      expect(parseDeviceGoneError("permission denied").gone).toBe(false);
    });

    it("reconnects once per 10s per device", async () => {
      const t = setup();
      t.reconnect.mockResolvedValue(ok);
      expect(await t.mon.reportDeviceError(err)).toBe(true);
      expect(await t.mon.reportDeviceError(err)).toBe(false);
      expect(t.reconnect).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(ERROR_RECONNECT_DEBOUNCE_MS);
      expect(await t.mon.reportDeviceError(err)).toBe(true);
      expect(t.reconnect).toHaveBeenCalledTimes(2);
    });

    it("uses the serial hint for 'device offline' and ignores unrelated errors", async () => {
      const t = setup();
      t.reconnect.mockResolvedValue(ok);
      expect(await t.mon.reportDeviceError("boom", "192.168.0.175:37317")).toBe(false);
      expect(await t.mon.reportDeviceError("device offline", "192.168.0.175:37317")).toBe(true);
    });

    it("ignores unknown or USB serials", async () => {
      const t = setup();
      expect(await t.mon.reportDeviceError("device 'ABC123' not found")).toBe(false);
      expect(await t.mon.reportDeviceError("device '10.0.0.9:5555' not found")).toBe(false);
      expect(t.reconnect).not.toHaveBeenCalled();
    });
  });
});

describe("badge state from live list", () => {
  it("shows connected only when the live list has the device", () => {
    expect(buildRows([live], [saved], []).savedRows[0].status).toBe("connected");
    expect(buildRows([], [saved], []).savedRows[0].status).toBe("offline");
    expect(buildRows([{ ...live, state: "offline" }], [saved], []).savedRows[0].status).toBe(
      "offline",
    );
  });
});
