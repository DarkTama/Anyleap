import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "./useAppStore";
import { DEFAULT_SETTINGS } from "@/lib/types";
import { DEFAULT_CONTROL_CONFIG } from "@/lib/controlConfig";
import { DEFAULT_APP_PREFS } from "@/lib/persist";
import type { SessionInfo } from "@/lib/types";

describe("useAppStore", () => {
  beforeEach(() => {
    useAppStore.setState({
      devices: [],
      sessions: [],
      savedDevices: [],
      settings: DEFAULT_SETTINGS,
      preset: "medium",
      controlConfig: DEFAULT_CONTROL_CONFIG,
      appPrefs: DEFAULT_APP_PREFS,
      error: null,
      deviceToggles: {},
      nicknames: {},
    });
  });

  describe("sessions management", () => {
    it("appends a new session and updates an existing session by id", () => {
      const s1: SessionInfo = {
        id: "s1",
        serial: "device-1",
        pid: 101,
        started_at: 1000,
        mode: "display",
      };
      const s2: SessionInfo = {
        id: "s2",
        serial: "device-2",
        pid: 102,
        started_at: 2000,
        mode: "display",
      };

      useAppStore.getState().upsertSession(s1);
      expect(useAppStore.getState().sessions).toEqual([s1]);

      useAppStore.getState().upsertSession(s2);
      expect(useAppStore.getState().sessions).toEqual([s1, s2]);

      const s1Updated: SessionInfo = {
        id: "s1",
        serial: "device-1",
        pid: 101,
        started_at: 9999,
        mode: "camera",
      };
      useAppStore.getState().upsertSession(s1Updated);
      expect(useAppStore.getState().sessions).toEqual([s1Updated, s2]);
    });

    it("removes session by id", () => {
      const s1: SessionInfo = {
        id: "s1",
        serial: "device-1",
        pid: 101,
        started_at: 1000,
        mode: "display",
      };
      const s2: SessionInfo = {
        id: "s2",
        serial: "device-2",
        pid: 102,
        started_at: 2000,
        mode: "display",
      };
      useAppStore.setState({ sessions: [s1, s2] });

      useAppStore.getState().removeSession("s1");
      expect(useAppStore.getState().sessions).toEqual([s2]);
    });
  });

  describe("nickname management", () => {
    it("stores trimmed nickname and removes entry when trimmed name is empty", () => {
      useAppStore.getState().setNickname("dev-1", "  My Phone  ");
      expect(useAppStore.getState().nicknames).toEqual({ "dev-1": "My Phone" });

      // Setting empty or whitespace removes key from map
      useAppStore.getState().setNickname("dev-1", "   ");
      expect(useAppStore.getState().nicknames).toEqual({});
    });
  });

  describe("device toggles", () => {
    it("merges toggle flags per serial without clobbering sibling keys", () => {
      useAppStore.getState().setDeviceToggle("dev-1", "screenOff", true);
      expect(useAppStore.getState().deviceToggles["dev-1"]).toEqual({ screenOff: true });

      useAppStore.getState().setDeviceToggle("dev-1", "asleep", true);
      expect(useAppStore.getState().deviceToggles["dev-1"]).toEqual({
        screenOff: true,
        asleep: true,
      });

      useAppStore.getState().setDeviceToggle("dev-2", "swipeScroll", true);
      expect(useAppStore.getState().deviceToggles["dev-1"]).toEqual({
        screenOff: true,
        asleep: true,
      });
      expect(useAppStore.getState().deviceToggles["dev-2"]).toEqual({
        swipeScroll: true,
      });
    });
  });
});
