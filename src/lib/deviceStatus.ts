import type { DeviceInfo, MdnsService, SavedDevice, SessionInfo } from "./types";

export type DeviceStatus = "offline" | "connected" | "mirroring";

export const MSG_NEEDS_REPAIR = "This phone needs to be paired again";

export function parseHostPort(serial: string): { host: string; port: number } | null {
  const idx = serial.lastIndexOf(":");
  if (idx <= 0) return null;
  const host = serial.slice(0, idx);
  const port = Number(serial.slice(idx + 1));
  if (!host || !Number.isInteger(port) || port <= 0 || port > 65535) return null;
  return { host, port };
}

export function isWifiSerial(serial: string): boolean {
  return parseHostPort(serial) !== null;
}

export function isLoopbackHost(host: string): boolean {
  const h = host.trim().toLowerCase();
  return h === "127.0.0.1" || h === "localhost" || h === "::1" || h === "[::1]";
}

function looksLikeIpOrEndpoint(value: string): boolean {
  if (!value) return false;
  if (parseHostPort(value)) return true;
  return /^\d{1,3}(\.\d{1,3}){3}$/.test(value.trim());
}

export function isGenericDeviceLabel(
  label: string,
  saved?: Pick<SavedDevice, "id" | "host" | "port" | "lastSerial">,
): boolean {
  const trimmed = label.trim();
  if (!trimmed) return true;
  if (looksLikeIpOrEndpoint(trimmed)) return true;
  if (saved) {
    if (trimmed === saved.host || trimmed === saved.lastSerial) return true;
    if (trimmed === `${saved.host}:${saved.port}`) return true;
    if (trimmed === saved.id && looksLikeIpOrEndpoint(saved.id)) return true;
  }
  return false;
}

export function deviceHardwareKey(device: DeviceInfo): string {
  return device.hw_serial || device.serial;
}

export function savedHardwareKey(saved: SavedDevice): string {
  return saved.hw_serial || saved.id || saved.host;
}

function stateRank(state: string): number {
  if (state === "device") return 3;
  if (state === "unauthorized" || state === "authorizing") return 2;
  return 1;
}

/**
 * Group/deduplicate live `DeviceInfo` records by `hw_serial` (preferring live USB
 * transport when both USB and Wi-Fi are connected) and drop dead offline `IP:port`
 * entries belonging to a live device on the same non-loopback host.
 */
export function dedupeDevices(devices: DeviceInfo[]): DeviceInfo[] {
  const liveHosts = new Map<string, Set<string>>();
  const liveHwSet = new Set<string>();

  for (const d of devices) {
    if (d.state === "device") {
      if (d.hw_serial) liveHwSet.add(d.hw_serial);
      const wifiEp = d.transports?.wifi ?? (isWifiSerial(d.serial) ? d.serial : null);
      if (wifiEp) {
        const hp = parseHostPort(wifiEp);
        if (hp && !isLoopbackHost(hp.host)) {
          let set = liveHosts.get(hp.host);
          if (!set) {
            set = new Set();
            liveHosts.set(hp.host, set);
          }
          set.add(wifiEp);
        }
      }
    }
  }

  const order: string[] = [];
  const groups = new Map<string, DeviceInfo[]>();

  for (const d of devices) {
    if (d.state === "offline" && isWifiSerial(d.serial)) {
      if (d.hw_serial && liveHwSet.has(d.hw_serial)) continue;
      const hp = parseHostPort(d.serial);
      if (hp && !isLoopbackHost(hp.host)) {
        const eps = liveHosts.get(hp.host);
        if (eps && !eps.has(d.serial)) continue;
      }
    }
    const key = deviceHardwareKey(d);
    let bucket = groups.get(key);
    if (!bucket) {
      bucket = [];
      groups.set(key, bucket);
      order.push(key);
    }
    bucket.push(d);
  }

  const out: DeviceInfo[] = [];
  for (const key of order) {
    const items = groups.get(key) ?? [];
    if (items.length === 1) {
      const only = items[0];
      const wifi = only.transports?.wifi ?? (isWifiSerial(only.serial) ? only.serial : null);
      const usb = only.transports?.usb ?? (!isWifiSerial(only.serial) ? only.serial : null);
      out.push({
        ...only,
        transports: { usb, wifi },
      });
      continue;
    }

    const usbCandidates = items
      .filter((d) => Boolean(d.transports?.usb) || !isWifiSerial(d.serial))
      .sort((a, b) => stateRank(b.state) - stateRank(a.state));
    const wifiCandidates = items
      .filter((d) => Boolean(d.transports?.wifi) || isWifiSerial(d.serial))
      .sort((a, b) => stateRank(b.state) - stateRank(a.state));

    const bestUsb = usbCandidates[0];
    const bestWifi = wifiCandidates[0];
    const usbLive = bestUsb?.state === "device";
    const wifiLive = bestWifi?.state === "device";

    const primary = usbLive
      ? bestUsb
      : wifiLive
        ? bestWifi
        : (bestUsb ?? bestWifi ?? items[0]);

    const usbSerial =
      bestUsb?.transports?.usb ??
      (bestUsb && !isWifiSerial(bestUsb.serial) ? bestUsb.serial : null);
    const wifiSerial =
      bestWifi?.transports?.wifi ??
      (bestWifi && isWifiSerial(bestWifi.serial) ? bestWifi.serial : null);

    out.push({
      serial: usbLive && usbSerial ? usbSerial : primary.serial,
      hw_serial: items.find((i) => i.hw_serial)?.hw_serial ?? null,
      state: primary.state,
      model: bestUsb?.model ?? bestWifi?.model ?? primary.model,
      product: bestUsb?.product ?? bestWifi?.product ?? primary.product,
      transports: {
        usb: usbSerial,
        wifi: wifiSerial,
      },
    });
  }

  return out;
}

/**
 * Match a saved device to a live one by `hw_serial` first, then by exact
 * transport serial, then by host prefix (wireless ports are dynamic).
 */
export function matchConnected(
  saved: SavedDevice,
  devices: DeviceInfo[],
): DeviceInfo | undefined {
  if (saved.hw_serial) {
    const byHw = devices.find((d) => d.hw_serial && d.hw_serial === saved.hw_serial);
    if (byHw) return byHw;
  }
  if (saved.transports?.usb) {
    const byUsb = devices.find(
      (d) => d.transports?.usb === saved.transports?.usb || d.serial === saved.transports?.usb,
    );
    if (byUsb) return byUsb;
  }
  if (saved.lastSerial) {
    const byLast = devices.find(
      (d) => d.transports?.wifi === saved.lastSerial || d.serial === saved.lastSerial,
    );
    if (byLast) return byLast;
  }
  if (saved.host) {
    const prefix = `${saved.host}:`;
    const byHost = devices.find(
      (d) => d.transports?.wifi?.startsWith(prefix) || d.serial.startsWith(prefix),
    );
    if (byHost) return byHost;
  }
  return undefined;
}

export function findSessionForDevice(
  target: { device?: DeviceInfo | null; saved?: SavedDevice | null; serial?: string | null },
  sessions: SessionInfo[],
): SessionInfo | undefined {
  const candidates = new Set<string>();
  if (target.serial) candidates.add(target.serial);
  if (target.device) {
    candidates.add(target.device.serial);
    if (target.device.transports?.usb) candidates.add(target.device.transports.usb);
    if (target.device.transports?.wifi) candidates.add(target.device.transports.wifi);
  }
  if (target.saved) {
    if (target.saved.lastSerial) candidates.add(target.saved.lastSerial);
    if (target.saved.transports?.usb) candidates.add(target.saved.transports.usb);
    if (target.saved.transports?.wifi) candidates.add(target.saved.transports.wifi);
    if (target.saved.host && target.saved.port) {
      candidates.add(`${target.saved.host}:${target.saved.port}`);
    }
  }
  return sessions.find((s) => candidates.has(s.serial));
}

/** Status of a live device (USB or wireless). */
export function deviceStatusOf(device: DeviceInfo, sessions: SessionInfo[]): DeviceStatus {
  if (findSessionForDevice({ device }, sessions)) return "mirroring";
  return device.state === "device" ? "connected" : "offline";
}

export interface SavedRow {
  saved: SavedDevice;
  device?: DeviceInfo; // the connected DeviceInfo, if matched
  status: DeviceStatus;
}

/**
 * Merge the live device list, saved devices, and running sessions into the two
 * sections rendered by Device Rack / Devices Tab. Deduplicates live devices by
 * `hw_serial` and drops dead `IP:port` entries belonging to a merged device.
 */
export function buildRows(
  devices: DeviceInfo[],
  savedDevices: SavedDevice[],
  sessions: SessionInfo[],
): { savedRows: SavedRow[]; otherDevices: DeviceInfo[] } {
  const mergedLive = dedupeDevices(devices);

  const savedRows: SavedRow[] = savedDevices.map((saved) => {
    const device = matchConnected(saved, mergedLive);
    return {
      saved,
      device,
      status: device ? deviceStatusOf(device, sessions) : "offline",
    };
  });

  const claimedSerials = new Set<string>();
  const claimedHw = new Set<string>();
  const claimedHosts = new Set<string>();

  for (const r of savedRows) {
    if (r.saved.hw_serial) claimedHw.add(r.saved.hw_serial);
    if (r.saved.host && !isLoopbackHost(r.saved.host)) claimedHosts.add(r.saved.host);
    if (r.device) {
      claimedSerials.add(r.device.serial);
      if (r.device.hw_serial) claimedHw.add(r.device.hw_serial);
      if (r.device.transports?.usb) claimedSerials.add(r.device.transports.usb);
      if (r.device.transports?.wifi) {
        claimedSerials.add(r.device.transports.wifi);
        const hp = parseHostPort(r.device.transports.wifi);
        if (hp && !isLoopbackHost(hp.host)) claimedHosts.add(hp.host);
      }
    }
  }

  const otherDevices = mergedLive.filter((d) => {
    if (claimedSerials.has(d.serial)) return false;
    if (d.hw_serial && claimedHw.has(d.hw_serial)) return false;
    if (d.state === "offline" && isWifiSerial(d.serial)) {
      const hp = parseHostPort(d.serial);
      if (hp && claimedHosts.has(hp.host)) return false;
    }
    return true;
  });

  return { savedRows, otherDevices };
}

/**
 * Migrate legacy saved devices (which may be keyed by `IP:port` or `host`) to
 * stable `hw_serial` identity when online or matched via mDNS, merge duplicates,
 * remove dead `IP:port` entries, and preserve the user's custom name & nicknames.
 */
export function migrateSavedDevices(
  savedDevices: SavedDevice[],
  liveDevices: DeviceInfo[],
  mdns: MdnsService[] = [],
  nicknames: Record<string, string> = {},
): {
  savedDevices: SavedDevice[];
  nicknames: Record<string, string>;
  changed: boolean;
  nicknamesChanged: boolean;
} {
  const mergedLive = dedupeDevices(liveDevices);
  const nextNicknames = { ...nicknames };
  let nicknamesChanged = false;

  const enriched = savedDevices.map((raw) => {
    // Legacy entries sometimes stored `host:port` inside `id` or `host`
    const idHp = parseHostPort(raw.id);
    const lastHp = parseHostPort(raw.lastSerial);
    const host = raw.host || idHp?.host || lastHp?.host || "";
    const port = raw.port || idHp?.port || lastHp?.port || 0;

    let hw =
      raw.hw_serial ??
      (!looksLikeIpOrEndpoint(raw.id) && raw.id !== host ? raw.id : null);

    const candidate: SavedDevice = {
      ...raw,
      hw_serial: hw,
      host,
      port,
      lastSerial: raw.lastSerial || (host && port ? `${host}:${port}` : raw.id),
    };

    const live = matchConnected(candidate, mergedLive);
    if (live?.hw_serial) {
      hw = live.hw_serial;
    } else if (!hw && host) {
      const mdnsMatch = mdns.find(
        (s) =>
          (s.serviceType === "_adb-tls-connect._tcp" || s.serviceType === "_adb._tcp") &&
          s.host === host &&
          Boolean(s.hwSerial),
      );
      if (mdnsMatch?.hwSerial) {
        hw = mdnsMatch.hwSerial;
      }
    }

    const liveWifi = live?.transports?.wifi ?? (live && isWifiSerial(live.serial) ? live.serial : null);
    const liveWifiHp = liveWifi ? parseHostPort(liveWifi) : null;
    const liveUsb = live?.transports?.usb ?? (live && !isWifiSerial(live.serial) ? live.serial : null);

    const mdnsConnect = hw
      ? mdns.find((s) => s.serviceType === "_adb-tls-connect._tcp" && s.hwSerial === hw)
      : host
        ? mdns.find((s) => s.serviceType === "_adb-tls-connect._tcp" && s.host === host)
        : undefined;

    const updatedHost = liveWifiHp?.host ?? (live?.state === "device" ? mdnsConnect?.host : undefined) ?? host;
    const updatedPort = liveWifiHp?.port ?? (live?.state === "device" ? mdnsConnect?.port : undefined) ?? port;
    const updatedWifi =
      liveWifi ??
      (updatedHost && updatedPort ? `${updatedHost}:${updatedPort}` : candidate.transports?.wifi ?? null);

    const updated: SavedDevice = {
      id: hw || (host && !isLoopbackHost(host) ? host : raw.id),
      hw_serial: hw ?? null,
      label: raw.label,
      host: updatedHost,
      port: updatedPort,
      lastSerial: liveWifi ?? (updatedHost && updatedPort ? `${updatedHost}:${updatedPort}` : candidate.lastSerial),
      transports: {
        usb: liveUsb ?? candidate.transports?.usb ?? null,
        wifi: updatedWifi,
      },
    };

    return { raw, updated, live };
  });

  // Group by hw_serial (when known) else non-loopback host else id
  const order: string[] = [];
  const groups = new Map<string, typeof enriched>();

  for (const item of enriched) {
    const { updated } = item;
    const groupKey =
      updated.hw_serial ||
      (updated.host && !isLoopbackHost(updated.host) ? `host:${updated.host}` : `id:${updated.id}`);
    let bucket = groups.get(groupKey);
    if (!bucket) {
      bucket = [];
      groups.set(groupKey, bucket);
      order.push(groupKey);
    }
    bucket.push(item);
  }

  const migrated: SavedDevice[] = [];
  for (const key of order) {
    const items = groups.get(key) ?? [];
    // Prefer entry that has a live match or newer/non-generic info
    const sorted = [...items].sort((a, b) => {
      const aLive = a.live?.state === "device" ? 1 : 0;
      const bLive = b.live?.state === "device" ? 1 : 0;
      if (aLive !== bLive) return bLive - aLive;
      const aHw = a.updated.hw_serial ? 1 : 0;
      const bHw = b.updated.hw_serial ? 1 : 0;
      return bHw - aHw;
    });

    const primary = sorted[0].updated;
    const liveMatch = sorted.find((i) => i.live)?.live;

    // Keep user's custom label if any merged entry had one
    let chosenLabel = "";
    for (const item of items) {
      if (!isGenericDeviceLabel(item.raw.label, item.raw)) {
        chosenLabel = item.raw.label.trim();
        break;
      }
    }
    if (!chosenLabel) {
      chosenLabel =
        liveMatch?.model ||
        liveMatch?.product ||
        primary.label ||
        primary.host ||
        primary.id;
    }

    const hw = primary.hw_serial ?? null;
    const canonicalId = hw || primary.id;

    // Migrate nicknames from any legacy key (IP:port, host, USB serial) to canonical hw_serial/id
    if (canonicalId) {
      const legacyKeys = new Set<string>();
      for (const item of items) {
        legacyKeys.add(item.raw.id);
        if (item.raw.host) legacyKeys.add(item.raw.host);
        if (item.raw.lastSerial) legacyKeys.add(item.raw.lastSerial);
        if (item.raw.host && item.raw.port) {
          legacyKeys.add(`${item.raw.host}:${item.raw.port}`);
        }
        if (item.raw.transports?.usb) legacyKeys.add(item.raw.transports.usb);
        if (item.raw.transports?.wifi) legacyKeys.add(item.raw.transports.wifi);
      }
      if (liveMatch) {
        legacyKeys.add(liveMatch.serial);
        if (liveMatch.transports?.usb) legacyKeys.add(liveMatch.transports.usb);
        if (liveMatch.transports?.wifi) legacyKeys.add(liveMatch.transports.wifi);
      }

      if (!nextNicknames[canonicalId]) {
        for (const lk of legacyKeys) {
          if (lk && lk !== canonicalId && nextNicknames[lk]) {
            nextNicknames[canonicalId] = nextNicknames[lk];
            nicknamesChanged = true;
            break;
          }
        }
      }
      // Clean up dead IP:port nickname keys once migrated to hw_serial
      if (hw) {
        for (const lk of legacyKeys) {
          if (lk && lk !== canonicalId && isWifiSerial(lk) && nextNicknames[lk]) {
            delete nextNicknames[lk];
            nicknamesChanged = true;
          }
        }
      }
    }

    migrated.push({
      id: canonicalId,
      hw_serial: hw,
      label: chosenLabel,
      host: primary.host,
      port: primary.port,
      lastSerial: primary.lastSerial,
      transports: primary.transports,
    });
  }

  const changed = JSON.stringify(migrated) !== JSON.stringify(savedDevices);
  return {
    savedDevices: migrated,
    nicknames: nextNicknames,
    changed,
    nicknamesChanged,
  };
}

export interface ResolvedDeviceTarget {
  device: DeviceInfo | null;
  saved: SavedDevice | null;
  activeSerial: string | null;
  hwSerial: string | null;
  isLive: boolean;
  isWireless: boolean;
  hasUsb: boolean;
  hasWifi: boolean;
}

/**
 * Resolve any device identifier (`hw_serial`, USB serial, `IP:port`, or `saved.id`)
 * to the unified live `DeviceInfo` + `SavedDevice` + preferred active transport serial.
 */
export function resolveDeviceTarget(
  key: string | null,
  devices: DeviceInfo[],
  savedDevices: SavedDevice[],
): ResolvedDeviceTarget {
  const mergedLive = dedupeDevices(devices);

  if (!key) {
    const firstDev = mergedLive[0] ?? null;
    const firstSaved = firstDev
      ? savedDevices.find((s) => matchConnected(s, [firstDev])) ?? null
      : (savedDevices[0] ?? null);
    const activeSerial =
      firstDev?.serial ??
      (firstSaved ? firstSaved.lastSerial || `${firstSaved.host}:${firstSaved.port}` : null);
    const hasUsb = Boolean(firstDev?.transports?.usb || (firstDev && !isWifiSerial(firstDev.serial)));
    const hasWifi = Boolean(
      firstDev?.transports?.wifi ||
        (firstDev && isWifiSerial(firstDev.serial)) ||
        (!firstDev && firstSaved),
    );
    return {
      device: firstDev,
      saved: firstSaved,
      activeSerial,
      hwSerial: firstDev?.hw_serial ?? firstSaved?.hw_serial ?? null,
      isLive: firstDev?.state === "device",
      isWireless: Boolean(
        (activeSerial && isWifiSerial(activeSerial)) || (!firstDev && firstSaved),
      ),
      hasUsb,
      hasWifi,
    };
  }

  let matchedDev: DeviceInfo | undefined = mergedLive.find(
    (d) =>
      d.serial === key ||
      d.hw_serial === key ||
      d.transports?.usb === key ||
      d.transports?.wifi === key,
  );

  let matchedSaved: SavedDevice | undefined = savedDevices.find(
    (s) =>
      s.id === key ||
      s.hw_serial === key ||
      s.lastSerial === key ||
      `${s.host}:${s.port}` === key ||
      s.host === key,
  );

  if (!matchedDev && matchedSaved) {
    matchedDev = matchConnected(matchedSaved, mergedLive);
  }
  if (matchedDev && !matchedSaved) {
    matchedSaved = savedDevices.find((s) => matchConnected(s, [matchedDev!]));
  }
  if (!matchedDev && !matchedSaved && isWifiSerial(key)) {
    const hp = parseHostPort(key);
    if (hp && !isLoopbackHost(hp.host)) {
      const prefix = `${hp.host}:`;
      matchedDev = mergedLive.find(
        (d) => d.transports?.wifi?.startsWith(prefix) || d.serial.startsWith(prefix),
      );
      if (matchedDev) {
        matchedSaved = savedDevices.find((s) => matchConnected(s, [matchedDev!]));
      }
    }
  }

  const device = matchedDev ?? null;
  const saved = matchedSaved ?? null;
  const activeSerial =
    device?.serial ?? (saved ? saved.lastSerial || `${saved.host}:${saved.port}` : null);
  const hwSerial = device?.hw_serial ?? saved?.hw_serial ?? null;
  const hasUsb = Boolean(device?.transports?.usb || (device && !isWifiSerial(device.serial)));
  const hasWifi = Boolean(
    device?.transports?.wifi ||
      (device && isWifiSerial(device.serial)) ||
      (!device && saved),
  );

  return {
    device,
    saved,
    activeSerial,
    hwSerial,
    isLive: device?.state === "device",
    isWireless: Boolean(
      (activeSerial && isWifiSerial(activeSerial)) || (!device && saved),
    ),
    hasUsb,
    hasWifi,
  };
}

/**
 * Resolve device nickname robustly across dynamic reconnect ports and USB/Wi-Fi transports.
 * Checks `hw_serial` first, then `saved.id` / `host` / `lastSerial` / active `serial`.
 */
export function resolveDeviceNickname(
  nicknames: Record<string, string>,
  target: { serial?: string; saved?: SavedDevice; device?: DeviceInfo },
): string | undefined {
  const { serial, saved, device } = target;
  const candidates = [
    device?.hw_serial,
    saved?.hw_serial,
    saved?.id,
    saved?.host,
    saved?.lastSerial,
    saved && saved.host && saved.port ? `${saved.host}:${saved.port}` : undefined,
    device?.serial,
    device?.transports?.usb,
    device?.transports?.wifi,
    serial,
  ];
  for (const key of candidates) {
    if (key && nicknames[key]) return nicknames[key];
  }
  return undefined;
}

/**
 * Returns the canonical, persistent storage key for saving a nickname.
 * Prefers stable `hw_serial`, then `saved.id` / `host`, then `serial`.
 */
export function getNicknameKey(target: {
  serial?: string;
  saved?: SavedDevice;
  device?: DeviceInfo;
}): string {
  if (target.device?.hw_serial) return target.device.hw_serial;
  if (target.saved?.hw_serial) return target.saved.hw_serial;
  if (target.saved) return target.saved.id || target.saved.host;
  return target.device?.serial ?? target.serial ?? "";
}

export function isAuthConnectError(msg: string): boolean {
  const low = msg.toLowerCase();
  return (
    low.includes("failed to authenticate") ||
    low.includes("unauthorized") ||
    low.includes("authorizing") ||
    low.includes("not paired") ||
    low.includes("rejected") ||
    low.includes("paired again")
  );
}

export type FrontendReconnectDecision =
  | { kind: "already_connected"; serial: string; host?: string; port?: number }
  | { kind: "reconnect_mdns"; host: string; port: number; hwSerial?: string | null }
  | { kind: "try_stored"; host: string; port: number }
  | { kind: "needs_repair"; message: string };

/** Pure reconnect decision helper for frontend tests and fallback logic. */
export function decideWirelessReconnect(
  saved: Pick<SavedDevice, "hw_serial" | "host" | "port">,
  liveDevices: DeviceInfo[],
  mdns: MdnsService[],
): FrontendReconnectDecision {
  const merged = dedupeDevices(liveDevices);
  const live = merged.find(
    (d) =>
      (saved.hw_serial && d.hw_serial === saved.hw_serial) ||
      (saved.host && (d.transports?.wifi?.startsWith(`${saved.host}:`) || d.serial.startsWith(`${saved.host}:`))),
  );
  if (live && live.state === "device" && live.transports?.wifi) {
    const hp = parseHostPort(live.transports.wifi);
    return {
      kind: "already_connected",
      serial: live.serial,
      host: hp?.host,
      port: hp?.port,
    };
  }
  if (live && live.state === "unauthorized" && !live.transports?.usb) {
    return { kind: "needs_repair", message: MSG_NEEDS_REPAIR };
  }

  const mdnsMatch =
    (saved.hw_serial
      ? mdns.find(
          (s) => s.serviceType === "_adb-tls-connect._tcp" && s.hwSerial === saved.hw_serial,
        )
      : undefined) ??
    (!saved.hw_serial && saved.host
      ? mdns.find((s) => s.serviceType === "_adb-tls-connect._tcp" && s.host === saved.host)
      : undefined);

  if (mdnsMatch) {
    return {
      kind: "reconnect_mdns",
      host: mdnsMatch.host,
      port: mdnsMatch.port,
      hwSerial: mdnsMatch.hwSerial ?? saved.hw_serial ?? null,
    };
  }

  if (saved.host && saved.port > 0) {
    return { kind: "try_stored", host: saved.host, port: saved.port };
  }
  return { kind: "needs_repair", message: MSG_NEEDS_REPAIR };
}
