import type { DeviceInfo, SavedDevice, SessionInfo } from "./types";

export type DeviceStatus = "offline" | "connected" | "mirroring";

/**
 * Match a saved device to a live one by **host prefix** — a wireless device's
 * connect port is dynamic, so never compare the saved port / lastSerial.
 */
export function matchConnected(
  saved: SavedDevice,
  devices: DeviceInfo[],
): DeviceInfo | undefined {
  return devices.find((d) => d.serial.startsWith(`${saved.host}:`));
}

/** Status of a live device (USB or wireless). */
export function deviceStatusOf(device: DeviceInfo, sessions: SessionInfo[]): DeviceStatus {
  if (sessions.some((s) => s.serial === device.serial)) return "mirroring";
  return device.state === "device" ? "connected" : "offline";
}

export interface SavedRow {
  saved: SavedDevice;
  device?: DeviceInfo; // the connected DeviceInfo, if matched
  status: DeviceStatus;
}

/**
 * Merge the live device list, saved devices, and running sessions into the two
 * sections the Devices tab renders. A connected saved device is deduped out of
 * `otherDevices` so it appears once (in Saved).
 */
export function buildRows(
  devices: DeviceInfo[],
  savedDevices: SavedDevice[],
  sessions: SessionInfo[],
): { savedRows: SavedRow[]; otherDevices: DeviceInfo[] } {
  const savedRows: SavedRow[] = savedDevices.map((saved) => {
    const device = matchConnected(saved, devices);
    return {
      saved,
      device,
      status: device ? deviceStatusOf(device, sessions) : "offline",
    };
  });

  const claimed = new Set(
    savedRows.map((r) => r.device?.serial).filter((s): s is string => Boolean(s)),
  );
  const otherDevices = devices.filter((d) => !claimed.has(d.serial));

  return { savedRows, otherDevices };
}

/**
 * Resolve device nickname robustly across dynamic reconnect ports.
 * Saved devices look up by persistent id / host first, falling back to
 * last-known serial or active serial. Unsaved devices look up by serial.
 */
export function resolveDeviceNickname(
  nicknames: Record<string, string>,
  target: { serial?: string; saved?: SavedDevice },
): string | undefined {
  const { serial, saved } = target;
  if (saved) {
    if (saved.id && nicknames[saved.id]) return nicknames[saved.id];
    if (saved.host && nicknames[saved.host]) return nicknames[saved.host];
    if (saved.lastSerial && nicknames[saved.lastSerial]) return nicknames[saved.lastSerial];
    const fallback = `${saved.host}:${saved.port}`;
    if (nicknames[fallback]) return nicknames[fallback];
  }
  if (serial && nicknames[serial]) {
    return nicknames[serial];
  }
  return undefined;
}

/**
 * Returns the canonical, persistent storage key for saving a nickname.
 * Saved devices use stable saved ID (host); ad-hoc devices use serial.
 */
export function getNicknameKey(target: { serial?: string; saved?: SavedDevice }): string {
  if (target.saved) {
    return target.saved.id || target.saved.host;
  }
  return target.serial ?? "";
}
