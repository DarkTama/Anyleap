import { useAppStore } from "../store/useAppStore";
import { surfaceAdbError } from "./adbError";
import {
  dedupeDevices,
  deviceHardwareKey,
  isAuthConnectError,
  isGenericDeviceLabel,
  matchConnected,
  migrateSavedDevices,
  MSG_NEEDS_REPAIR,
  resolveDeviceTarget,
  savedHardwareKey,
} from "./deviceStatus";
import { replaceNicknames, replaceSavedDevices } from "./settingsStore";
import { discoverWireless, listDevices, reconnectWireless } from "./tauri";
import type { DeviceInfo, MdnsService, ReconnectResult, SavedDevice } from "./types";

type SyncResult = {
  devices: DeviceInfo[];
  savedDevices: SavedDevice[];
  mdns: MdnsService[];
};

type RefreshResult = SyncResult & {
  results: ReconnectResult[];
};

let inFlightSync: Promise<SyncResult> | null = null;
let inFlightRefresh: Promise<RefreshResult> | null = null;

export function repairKeyFor(target: {
  saved?: SavedDevice | null;
  device?: DeviceInfo | null;
  serial?: string | null;
}): string {
  if (target.device) return deviceHardwareKey(target.device);
  if (target.saved) return savedHardwareKey(target.saved);
  return target.serial ?? "";
}

function clearLiveRepairFlags(
  devices: DeviceInfo[],
  savedDevices: SavedDevice[],
  repairNeeded: Record<string, string>,
  setRepairNeeded: (key: string, msg: string | null) => void,
): void {
  if (Object.keys(repairNeeded).length === 0) return;
  for (const d of devices) {
    if (d.state !== "device") continue;
    const keys = new Set<string>();
    keys.add(deviceHardwareKey(d));
    keys.add(d.serial);
    if (d.hw_serial) keys.add(d.hw_serial);
    if (d.transports?.usb) keys.add(d.transports.usb);
    if (d.transports?.wifi) keys.add(d.transports.wifi);

    const matchedSaved = savedDevices.find((s) => matchConnected(s, [d]));
    if (matchedSaved) {
      keys.add(savedHardwareKey(matchedSaved));
      keys.add(matchedSaved.id);
      if (matchedSaved.host) keys.add(matchedSaved.host);
      if (matchedSaved.lastSerial) keys.add(matchedSaved.lastSerial);
    }

    for (const k of keys) {
      if (k && repairNeeded[k]) {
        setRepairNeeded(k, null);
      }
    }
  }
}

async function runSyncDevicesAndMigrate(
  preloadedDevices?: DeviceInfo[],
  preloadedMdns?: MdnsService[],
): Promise<SyncResult> {
  // Run sequentially rather than in parallel so startup ADB commands never
  // race on a cold server or double-spawn discovery unnecessarily.
  const rawDevices = preloadedDevices ?? (await listDevices());
  const mdns = preloadedMdns ?? (await discoverWireless().catch(() => []));

  const devices = dedupeDevices(rawDevices);
  const store = useAppStore.getState();

  const mig = migrateSavedDevices(store.savedDevices, devices, mdns, store.nicknames);
  let nextSaved = store.savedDevices;
  if (mig.changed) {
    nextSaved = await replaceSavedDevices(mig.savedDevices).catch(() => mig.savedDevices);
    useAppStore.getState().setSavedDevices(nextSaved);
  }
  if (mig.nicknamesChanged) {
    const nextNicks = await replaceNicknames(mig.nicknames).catch(() => mig.nicknames);
    useAppStore.getState().setNicknames(nextNicks);
  }

  const latest = useAppStore.getState();
  latest.setDevices(devices);
  clearLiveRepairFlags(devices, nextSaved, latest.repairNeeded, latest.setRepairNeeded);

  const resolved = resolveDeviceTarget(latest.selectedSerial, devices, nextSaved);
  if (
    resolved.activeSerial &&
    resolved.activeSerial !== latest.selectedSerial &&
    (resolved.device || resolved.saved || !latest.selectedSerial)
  ) {
    latest.setSelectedSerial(resolved.activeSerial);
  }

  return { devices, savedDevices: nextSaved, mdns };
}

/**
 * Fetch current adb devices + mDNS connect services, deduplicate live transports by
 * hardware serial, migrate any legacy IP:port saved entries/nicknames, and keep
 * `selectedSerial` pointing at the active transport for the selected phone.
 * Concurrent calls without preloaded snapshots coalesce onto the in-flight sync.
 */
export function syncDevicesAndMigrate(
  preloadedDevices?: DeviceInfo[],
  preloadedMdns?: MdnsService[],
): Promise<SyncResult> {
  if (preloadedDevices || preloadedMdns) {
    return runSyncDevicesAndMigrate(preloadedDevices, preloadedMdns);
  }
  if (inFlightSync) {
    return inFlightSync;
  }
  inFlightSync = runSyncDevicesAndMigrate().finally(() => {
    inFlightSync = null;
  });
  return inFlightSync;
}

function applyReconnectedTransport(
  savedDevices: SavedDevice[],
  target: SavedDevice,
  res: ReconnectResult,
): { next: SavedDevice[]; changed: boolean } {
  if (!res.host && !res.port && !res.hwSerial && !res.serial) {
    return { next: savedDevices, changed: false };
  }
  let changed = false;
  const next = savedDevices.map((s) => {
    const sameEntry =
      s.id === target.id ||
      (target.hw_serial && s.hw_serial === target.hw_serial) ||
      (target.host && s.host === target.host && s.port === target.port);
    if (!sameEntry) return s;

    const hwSerial = res.hwSerial ?? s.hw_serial;
    const host = res.host ?? s.host;
    const port = res.port ?? s.port;
    const wifiEndpoint =
      res.serial && res.serial.includes(":")
        ? res.serial
        : host && port
          ? `${host}:${port}`
          : s.lastSerial;

    const updated: SavedDevice = {
      ...s,
      id: hwSerial || s.id,
      hw_serial: hwSerial ?? s.hw_serial,
      label: isGenericDeviceLabel(s.label, s) && hwSerial ? s.label : s.label,
      host,
      port,
      lastSerial: wifiEndpoint,
      transports: {
        usb: s.transports?.usb ?? null,
        wifi: wifiEndpoint ?? s.transports?.wifi ?? null,
      },
    };
    if (JSON.stringify(updated) !== JSON.stringify(s)) {
      changed = true;
    }
    return updated;
  });
  return { next, changed };
}

/**
 * Reconnect a saved wireless device: tries its stored IP:port and falls back to
 * `_adb-tls-connect` mDNS matching `hw_serial`. Surfaces `"This phone needs to be paired again"`
 * only when no mDNS match exists or adb rejects authentication.
 */
export async function reconnectSavedDevice(
  saved: SavedDevice,
  opts?: { silent?: boolean; skipSync?: boolean },
): Promise<ReconnectResult> {
  const silent = opts?.silent ?? false;
  const key = savedHardwareKey(saved);

  try {
    const res = await reconnectWireless({
      hwSerial: saved.hw_serial ?? null,
      host: saved.host,
      port: saved.port,
    });

    const store = useAppStore.getState();

    if (res.status === "already_connected" || res.status === "reconnected") {
      store.setRepairNeeded(key, null);
      if (saved.id && saved.id !== key) store.setRepairNeeded(saved.id, null);
      if (res.hwSerial) store.setRepairNeeded(res.hwSerial, null);

      const { next, changed } = applyReconnectedTransport(store.savedDevices, saved, res);
      if (changed) {
        const persisted = await replaceSavedDevices(next).catch(() => next);
        useAppStore.getState().setSavedDevices(persisted);
      }

      if (!opts?.skipSync) {
        await syncDevicesAndMigrate().catch(() => {});
      }
      return res;
    }

    const msg = res.message || MSG_NEEDS_REPAIR;
    store.setRepairNeeded(key, msg);
    if (!silent) {
      store.setError(msg);
    }
    return { ...res, message: msg };
  } catch (e) {
    const rawMsg = String(e);
    const store = useAppStore.getState();
    if (isAuthConnectError(rawMsg)) {
      store.setRepairNeeded(key, MSG_NEEDS_REPAIR);
      if (!silent) store.setError(MSG_NEEDS_REPAIR);
      return {
        status: "needs_repair",
        serial: null,
        host: null,
        port: null,
        hwSerial: saved.hw_serial ?? null,
        message: MSG_NEEDS_REPAIR,
      };
    }
    if (!silent) {
      surfaceAdbError(rawMsg, store.setError);
    }
    return {
      status: "needs_repair",
      serial: null,
      host: null,
      port: null,
      hwSerial: saved.hw_serial ?? null,
      message: rawMsg,
    };
  }
}

async function runRefreshAndReconnectDevices(opts?: {
  silentRepair?: boolean;
}): Promise<RefreshResult> {
  const initial = await syncDevicesAndMigrate();
  const offlineSaved = initial.savedDevices.filter((s) => {
    if (!s.host) return false;
    const live = matchConnected(s, initial.devices);
    return !live || live.state !== "device";
  });

  if (offlineSaved.length === 0) {
    return { ...initial, results: [] };
  }

  const silent = opts?.silentRepair ?? true;
  const settled = await Promise.allSettled(
    offlineSaved.map((s) => reconnectSavedDevice(s, { silent, skipSync: true })),
  );
  const results: ReconnectResult[] = settled
    .filter((r): r is PromiseFulfilledResult<ReconnectResult> => r.status === "fulfilled")
    .map((r) => r.value);

  const finalSync = await syncDevicesAndMigrate();
  return { ...finalSync, results };
}

/**
 * Refresh adb devices + migrate saved devices, auto-reconnect any offline saved
 * wireless devices via mDNS / stored endpoint, and re-sync the merged device list.
 * Concurrent calls coalesce onto the active refresh promise.
 */
export function refreshAndReconnectDevices(opts?: {
  silentRepair?: boolean;
}): Promise<RefreshResult> {
  if (inFlightRefresh) {
    return inFlightRefresh;
  }
  inFlightRefresh = runRefreshAndReconnectDevices(opts).finally(() => {
    inFlightRefresh = null;
  });
  return inFlightRefresh;
}
