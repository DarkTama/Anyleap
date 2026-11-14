import { describe, it, expect } from "vitest";
import {
  matchConnected,
  deviceStatusOf,
  buildRows,
  resolveDeviceNickname,
  getNicknameKey,
} from "./deviceStatus";
import type { DeviceInfo, SavedDevice, SessionInfo } from "./types";

describe("deviceStatus", () => {
  describe("matchConnected", () => {
    it("matches live device by host prefix ignoring dynamic port", () => {
      const saved: SavedDevice = {
        id: "dev-1",
        label: "Pixel 7",
        host: "192.168.1.50",
        port: 5555,
        lastSerial: "192.168.1.50:5555",
      };

      const liveDevices: DeviceInfo[] = [
        {
          serial: "192.168.1.50:41235",
          state: "device",
          model: "Pixel 7",
          product: "panther",
        },
        {
          serial: "192.168.1.51:39999",
          state: "device",
          model: "Galaxy S23",
          product: "kalama",
        },
      ];

      const match = matchConnected(saved, liveDevices);
      expect(match).toBeDefined();
      expect(match?.serial).toBe("192.168.1.50:41235");
    });

    it("returns undefined when host does not match with colon boundary", () => {
      const saved: SavedDevice = {
        id: "dev-1",
        label: "Pixel 7",
        host: "192.168.1.5",
        port: 5555,
        lastSerial: "192.168.1.5:5555",
      };

      const liveDevices: DeviceInfo[] = [
        {
          serial: "192.168.1.50:41235",
          state: "device",
          model: "Pixel 7",
          product: "panther",
        },
      ];

      expect(matchConnected(saved, liveDevices)).toBeUndefined();
    });
  });

  describe("deviceStatusOf", () => {
    const baseDevice: DeviceInfo = {
      serial: "192.168.1.50:5555",
      state: "device",
      model: "Pixel 7",
      product: "panther",
    };

    it("returns 'mirroring' when an active session matches the device serial", () => {
      const sessions: SessionInfo[] = [
        {
          id: "sess-1",
          serial: "192.168.1.50:5555",
          pid: 1234,
          started_at: 100,
          mode: "display",
        },
      ];
      expect(deviceStatusOf(baseDevice, sessions)).toBe("mirroring");
    });

    it("returns 'connected' when state is 'device' and no session exists", () => {
      expect(deviceStatusOf(baseDevice, [])).toBe("connected");
    });

    it("returns 'offline' when device state is unauthorized or offline", () => {
      const unauthorizedDevice: DeviceInfo = {
        ...baseDevice,
        state: "unauthorized",
      };
      expect(deviceStatusOf(unauthorizedDevice, [])).toBe("offline");
    });
  });

  describe("buildRows", () => {
    it("deduplicates claimed saved devices from otherDevices and maps statuses", () => {
      const saved: SavedDevice = {
        id: "saved-1",
        label: "My Tablet",
        host: "192.168.1.10",
        port: 5555,
        lastSerial: "192.168.1.10:5555",
      };

      const matchedLive: DeviceInfo = {
        serial: "192.168.1.10:34567",
        state: "device",
        model: "Tab S8",
        product: "gts8",
      };

      const unclaimedUsb: DeviceInfo = {
        serial: "USB123456",
        state: "device",
        model: "Pixel 6",
        product: "oriole",
      };

      const sessions: SessionInfo[] = [
        {
          id: "s1",
          serial: "192.168.1.10:34567",
          pid: 5678,
          started_at: 100,
          mode: "display",
        },
      ];

      const { savedRows, otherDevices } = buildRows(
        [matchedLive, unclaimedUsb],
        [saved],
        sessions,
      );

      // savedRow verification
      expect(savedRows).toHaveLength(1);
      expect(savedRows[0].saved.id).toBe("saved-1");
      expect(savedRows[0].device?.serial).toBe("192.168.1.10:34567");
      expect(savedRows[0].status).toBe("mirroring");

      // deduplication verification: matchedLive claimed, only unclaimedUsb in otherDevices
      expect(otherDevices).toHaveLength(1);
      expect(otherDevices[0].serial).toBe("USB123456");
    });

    it("marks saved device as offline when not present in live devices", () => {
      const saved: SavedDevice = {
        id: "saved-offline",
        label: "Old Phone",
        host: "192.168.1.99",
        port: 5555,
        lastSerial: "192.168.1.99:5555",
      };

      const { savedRows, otherDevices } = buildRows([], [saved], []);

      expect(savedRows).toHaveLength(1);
      expect(savedRows[0].device).toBeUndefined();
      expect(savedRows[0].status).toBe("offline");
      expect(otherDevices).toHaveLength(0);
    });
  });

  describe("nickname resolution", () => {
    const saved: SavedDevice = {
      id: "192.168.1.50",
      label: "Living Room Tablet",
      host: "192.168.1.50",
      port: 5555,
      lastSerial: "192.168.1.50:5555",
    };

    it("resolves nickname by saved id even when dynamic connect port changes", () => {
      const nicknames = { "192.168.1.50": "Tablet Mini" };

      // Reconnected on new dynamic port 42135
      const nickname = resolveDeviceNickname(nicknames, {
        saved,
        serial: "192.168.1.50:42135",
      });

      expect(nickname).toBe("Tablet Mini");
    });

    it("resolves nickname from legacy host:port key if id key is absent", () => {
      const nicknames = { "192.168.1.50:5555": "Legacy Name" };

      const nickname = resolveDeviceNickname(nicknames, {
        saved,
        serial: "192.168.1.50:42135",
      });

      expect(nickname).toBe("Legacy Name");
    });

    it("resolves unsaved USB device nickname by serial", () => {
      const nicknames = { "USB_SERIAL_123": "My USB Phone" };

      const nickname = resolveDeviceNickname(nicknames, {
        serial: "USB_SERIAL_123",
      });

      expect(nickname).toBe("My USB Phone");
    });

    it("returns stable key for saving nicknames", () => {
      expect(getNicknameKey({ saved })).toBe("192.168.1.50");
      expect(getNicknameKey({ serial: "USB_SERIAL_123" })).toBe("USB_SERIAL_123");
    });
  });
});

import {
  dedupeDevices,
  migrateSavedDevices,
  resolveDeviceTarget,
  decideWirelessReconnect,
  MSG_NEEDS_REPAIR,
} from "./deviceStatus";
import type { MdnsService } from "./types";

describe("hw_serial grouping, migration, and wireless reconnect", () => {
  it("dedupeDevices merges USB and Wi-Fi for the same hw_serial, preferring USB", () => {
    const merged = dedupeDevices([
      {
        serial: "192.168.0.175:46391",
        hw_serial: "140892554J008528",
        state: "device",
        model: "Pixel 8",
        product: null,
      },
      {
        serial: "140892554J008528",
        hw_serial: "140892554J008528",
        state: "device",
        model: "Pixel 8",
        product: null,
      },
    ]);
    expect(merged).toHaveLength(1);
    expect(merged[0].serial).toBe("140892554J008528");
    expect(merged[0].hw_serial).toBe("140892554J008528");
    expect(merged[0].transports).toEqual({
      usb: "140892554J008528",
      wifi: "192.168.0.175:46391",
    });
  });

  it("dedupeDevices drops dead offline IP:port entries belonging to a live device", () => {
    const merged = dedupeDevices([
      {
        serial: "192.168.0.175:40921",
        hw_serial: null,
        state: "offline",
        model: null,
        product: null,
      },
      {
        serial: "192.168.0.175:46391",
        hw_serial: "140892554J008528",
        state: "device",
        model: "Pixel 8",
        product: null,
      },
    ]);
    expect(merged).toHaveLength(1);
    expect(merged[0].serial).toBe("192.168.0.175:46391");
  });

  it("migrateSavedDevices resolves hw_serial, merges IP:port duplicates, keeps custom name and nickname", () => {
    const legacySaved: SavedDevice[] = [
      {
        id: "192.168.0.175:40921",
        label: "192.168.0.175:40921",
        host: "192.168.0.175",
        port: 40921,
        lastSerial: "192.168.0.175:40921",
      },
      {
        id: "192.168.0.175",
        label: "Tama Pixel",
        host: "192.168.0.175",
        port: 46391,
        lastSerial: "192.168.0.175:46391",
      },
    ];
    const live: DeviceInfo[] = [
      {
        serial: "192.168.0.175:46391",
        hw_serial: "140892554J008528",
        state: "device",
        model: "Pixel 8",
        product: null,
        transports: { usb: null, wifi: "192.168.0.175:46391" },
      },
    ];
    const result = migrateSavedDevices(legacySaved, live, [], {
      "192.168.0.175:40921": "Studio Phone",
    });

    expect(result.changed).toBe(true);
    expect(result.savedDevices).toHaveLength(1);
    expect(result.savedDevices[0]).toMatchObject({
      id: "140892554J008528",
      hw_serial: "140892554J008528",
      label: "Tama Pixel",
      host: "192.168.0.175",
      port: 46391,
      lastSerial: "192.168.0.175:46391",
    });
    expect(result.nicknamesChanged).toBe(true);
    expect(result.nicknames["140892554J008528"]).toBe("Studio Phone");
  });

  it("buildRows groups saved device plugged into USB + Wi-Fi into ONE row without duplication", () => {
    const saved: SavedDevice[] = [
      {
        id: "140892554J008528",
        hw_serial: "140892554J008528",
        label: "Work Phone",
        host: "192.168.0.175",
        port: 46391,
        lastSerial: "192.168.0.175:46391",
      },
    ];
    const live: DeviceInfo[] = [
      {
        serial: "140892554J008528",
        hw_serial: "140892554J008528",
        state: "device",
        model: null,
        product: null,
      },
      {
        serial: "192.168.0.175:46391",
        hw_serial: "140892554J008528",
        state: "device",
        model: null,
        product: null,
      },
    ];
    const { savedRows, otherDevices } = buildRows(live, saved, []);
    expect(savedRows).toHaveLength(1);
    expect(savedRows[0].device?.serial).toBe("140892554J008528");
    expect(savedRows[0].status).toBe("connected");
    expect(otherDevices).toHaveLength(0);
  });

  it("resolveDeviceTarget follows stale port selection to new live Wi-Fi port or USB", () => {
    const saved: SavedDevice[] = [
      {
        id: "140892554J008528",
        hw_serial: "140892554J008528",
        label: "Pixel",
        host: "192.168.0.175",
        port: 46391,
        lastSerial: "192.168.0.175:46391",
      },
    ];
    const live: DeviceInfo[] = [
      {
        serial: "192.168.0.175:46391",
        hw_serial: "140892554J008528",
        state: "device",
        model: null,
        product: null,
        transports: { usb: null, wifi: "192.168.0.175:46391" },
      },
    ];
    const target = resolveDeviceTarget("192.168.0.175:40921", live, saved);
    expect(target.isLive).toBe(true);
    expect(target.activeSerial).toBe("192.168.0.175:46391");
    expect(target.hwSerial).toBe("140892554J008528");

    expect(
      resolveDeviceNickname(
        { "140892554J008528": "Daily" },
        { saved: target.saved!, device: target.device!, serial: target.activeSerial! },
      ),
    ).toBe("Daily");
  });

  it("decideWirelessReconnect picks new mDNS port by hw_serial and prompts re-pair only when needed", () => {
    const saved: SavedDevice = {
      id: "140892554J008528",
      hw_serial: "140892554J008528",
      label: "Pixel",
      host: "192.168.0.175",
      port: 40921,
      lastSerial: "192.168.0.175:40921",
    };
    const mdns: MdnsService[] = [
      {
        name: "adb-140892554J008528-Exhc24",
        serviceType: "_adb-tls-connect._tcp",
        host: "192.168.0.175",
        port: 46391,
        hwSerial: "140892554J008528",
      },
    ];
    expect(decideWirelessReconnect(saved, [], mdns)).toEqual({
      kind: "reconnect_mdns",
      host: "192.168.0.175",
      port: 46391,
      hwSerial: "140892554J008528",
    });

    const unauthorized = decideWirelessReconnect(
      saved,
      [
        {
          serial: "192.168.0.175:40921",
          hw_serial: "140892554J008528",
          state: "unauthorized",
          model: null,
          product: null,
        },
      ],
      [],
    );
    expect(unauthorized).toEqual({
      kind: "needs_repair",
      message: MSG_NEEDS_REPAIR,
    });
  });
});
