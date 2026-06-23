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
