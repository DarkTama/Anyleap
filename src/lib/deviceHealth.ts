import { useAppStore } from "../store/useAppStore";
import {
  dedupeDevices,
  findSessionForDevice,
  isWifiSerial,
  matchConnected,
  parseHostPort,
  savedHardwareKey,
} from "./deviceStatus";
import { isDeviceSyncBusy, reconnectSavedDevice } from "./deviceSync";
import { listDevices } from "./tauri";
import type { DeviceInfo, ReconnectResult, SavedDevice, SessionInfo } from "./types";

export const HEALTH_CHECK_INTERVAL_MS = 15_000;
/** Retry delays after consecutive failed reconnects; the last value is the cap. */
export const RECONNECT_BACKOFF_MS = [15_000, 30_000, 60_000];
export const ERROR_RECONNECT_DEBOUNCE_MS = 10_000;

export function backoffDelayMs(failures: number): number {
  if (failures <= 0) return 0;
  return RECONNECT_BACKOFF_MS[Math.min(failures, RECONNECT_BACKOFF_MS.length) - 1];
}

/** True for adb errors meaning the transport is gone ("device 'x' not found" / "device offline"). */
export function parseDeviceGoneError(err: unknown): { gone: boolean; serial: string | null } {
  const msg = err instanceof Error ? err.message : String(err ?? "");
  const m = msg.match(/device '([^']+)' not found/i);
  if (m) return { gone: true, serial: m[1] };
  if (/device offline/i.test(msg)) return { gone: true, serial: null };
  return { gone: false, serial: null };
}

export function isSavedWireless(saved: SavedDevice): boolean {
  return Boolean(saved.host) && saved.port > 0;
}

function findSavedForSerial(serial: string, saved: SavedDevice[]): SavedDevice | undefined {
  const hp = parseHostPort(serial);
  return saved.find(
    (s) =>
      isSavedWireless(s) &&
      (s.lastSerial === serial ||
        s.transports?.wifi === serial ||
        `${s.host}:${s.port}` === serial ||
        (hp !== null && s.host === hp.host)),
  );
}

export interface HealthDeps {
  listDevices: () => Promise<DeviceInfo[]>;
  reconnect: (saved: SavedDevice) => Promise<ReconnectResult>;
  getState: () => { savedDevices: SavedDevice[]; sessions: SessionInfo[] };
  setLiveDevices: (devices: DeviceInfo[]) => void;
  setReconnecting: (key: string, on: boolean) => void;
  isBusy: () => boolean;
  isHidden: () => boolean;
  now: () => number;
}

export function createDeviceHealthMonitor(deps: HealthDeps) {
  let timer: ReturnType<typeof setInterval> | null = null;
  let running = false;
  const failures = new Map<string, number>();
  const nextAttemptAt = new Map<string, number>();
  const lastErrorTrigger = new Map<string, number>();
  const inFlight = new Set<string>();

  async function reconnectOne(saved: SavedDevice): Promise<boolean> {
    const key = savedHardwareKey(saved);
    if (inFlight.has(key)) return false;
    inFlight.add(key);
    deps.setReconnecting(key, true);
    let ok = false;
    try {
      const res = await deps.reconnect(saved);
      ok = res.status === "already_connected" || res.status === "reconnected";
    } catch {
      ok = false;
    } finally {
      inFlight.delete(key);
      deps.setReconnecting(key, false);
    }
    if (ok) {
      failures.delete(key);
      nextAttemptAt.delete(key);
    } else {
      const n = (failures.get(key) ?? 0) + 1;
      failures.set(key, n);
      nextAttemptAt.set(key, deps.now() + backoffDelayMs(n));
    }
    return ok;
  }

  async function tick(): Promise<void> {
    if (running || deps.isBusy() || deps.isHidden()) return;
    const wireless = deps.getState().savedDevices.filter(isSavedWireless);
    if (wireless.length === 0) return;
    running = true;
    try {
      const live = dedupeDevices(await deps.listDevices());
      deps.setLiveDevices(live);
      // Re-read after the await: sessions/saved list may have changed meanwhile.
      const { savedDevices, sessions } = deps.getState();
      for (const saved of savedDevices.filter(isSavedWireless)) {
        const key = savedHardwareKey(saved);
        const match = matchConnected(saved, live);
        if (match && match.state === "device") {
          failures.delete(key);
          nextAttemptAt.delete(key);
          continue;
        }
        if (findSessionForDevice({ saved, device: match }, sessions)) continue;
        if (deps.now() < (nextAttemptAt.get(key) ?? 0)) continue;
        if (deps.isBusy()) break;
        await reconnectOne(saved);
      }
    } catch {
      // adb server hiccup: try again next tick.
    } finally {
      running = false;
    }
  }

  /** Reconnect a saved wireless phone after an adb "device not found/offline" error. */
  async function reportDeviceError(err: unknown, serialHint?: string | null): Promise<boolean> {
    const parsed = parseDeviceGoneError(err);
    if (!parsed.gone) return false;
    const serial = parsed.serial ?? serialHint ?? null;
    if (!serial || !isWifiSerial(serial)) return false;
    const { savedDevices, sessions } = deps.getState();
    const saved = findSavedForSerial(serial, savedDevices);
    if (!saved) return false;
    if (findSessionForDevice({ saved, serial }, sessions)) return false;
    const key = savedHardwareKey(saved);
    const now = deps.now();
    const last = lastErrorTrigger.get(key);
    if (last !== undefined && now - last < ERROR_RECONNECT_DEBOUNCE_MS) return false;
    lastErrorTrigger.set(key, now);
    return reconnectOne(saved);
  }

  function start(): void {
    if (timer) return;
    timer = setInterval(() => void tick(), HEALTH_CHECK_INTERVAL_MS);
  }
  function stop(): void {
    if (timer) clearInterval(timer);
    timer = null;
  }

  return { start, stop, tick, reportDeviceError };
}

export const deviceHealth = createDeviceHealthMonitor({
  listDevices: () => listDevices(),
  reconnect: (saved) => reconnectSavedDevice(saved, { silent: true }),
  getState: () => useAppStore.getState(),
  setLiveDevices: (devices) => {
    const st = useAppStore.getState();
    if (JSON.stringify(st.devices) !== JSON.stringify(devices)) st.setDevices(devices);
  },
  setReconnecting: (key, on) => useAppStore.getState().setReconnecting(key, on),
  isBusy: () => isDeviceSyncBusy(),
  isHidden: () => typeof document !== "undefined" && document.hidden,
  now: () => Date.now(),
});

/** Fire-and-forget helper for call sites; resolves true when a reconnect succeeded. */
export function reportDeviceError(err: unknown, serialHint?: string | null): Promise<boolean> {
  return deviceHealth.reportDeviceError(err, serialHint);
}
