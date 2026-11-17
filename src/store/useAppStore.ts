import { create } from "zustand";
import type {
  BatteryInfo,
  DeviceDetails,
  DeviceInfo,
  SavedDevice,
  SessionInfo,
} from "@/lib/types";
import {
  defaultModeDefaults,
  type MirrorMode,
  type ModeDefaultsV2,
  type ModeSettings,
  type CameraModeSettings,
} from "@/lib/modeSettings";
import type { ControlConfig } from "@/lib/controlConfig";
import { DEFAULT_CONTROL_CONFIG } from "@/lib/controlConfig";
import type { AppPrefs } from "@/lib/persist";
import { DEFAULT_APP_PREFS } from "@/lib/persist";
import type { StudioMode } from "@/components/deck/LaunchMatrix";

interface AppStore {
  devices: DeviceInfo[];
  sessions: SessionInfo[];
  savedDevices: SavedDevice[];
  /** Per-mode mirroring defaults (SPEC-28). Launch settings are resolved from these. */
  modeDefaults: ModeDefaultsV2;
  /** One-time "audio is now on" notice from the migration is pending. */
  modeDefaultsNotice: boolean;
  /** Mode each running session was launched from, keyed by session id (SPEC-28 task 1.6). */
  sessionModes: Record<string, MirrorMode>;
  controlConfig: ControlConfig;
  appPrefs: AppPrefs;
  error: string | null;
  deviceToggles: Record<string, { screenOff?: boolean; asleep?: boolean; swipeScroll?: boolean }>;
  nicknames: Record<string, string>;
  selectedSerial: string | null;
  currentMode: StudioMode;
  deviceDetailsCache: Record<string, DeviceDetails>;
  batteryMap: Record<string, BatteryInfo>;
  doctorOpen: boolean;
  doctorSerial: string | null;
  timelineOpen: boolean;

  setNicknames: (n: Record<string, string>) => void;
  setNickname: (serial: string, name: string) => void;
  setSelectedSerial: (serial: string | null) => void;
  setCurrentMode: (mode: StudioMode) => void;
  setDeviceDetails: (serial: string, details: DeviceDetails) => void;
  setBatteryInfo: (serial: string, info: BatteryInfo) => void;
  openDoctor: (serial?: string | null) => void;
  closeDoctor: () => void;
  setTimelineOpen: (open: boolean) => void;
  repairNeeded: Record<string, string>;
  /** Saved-device keys currently being auto-reconnected by the health monitor. */
  reconnecting: Record<string, boolean>;
  setReconnecting: (key: string, on: boolean) => void;
  setRepairNeeded: (key: string, msg: string | null) => void;

  setDevices: (d: DeviceInfo[]) => void;
  setSessions: (s: SessionInfo[]) => void;
  upsertSession: (s: SessionInfo) => void;
  removeSession: (id: string) => void;
  setSavedDevices: (d: SavedDevice[]) => void;
  setModeDefaults: (d: ModeDefaultsV2, notice?: boolean) => void;
  setModeDefaultsNotice: (pending: boolean) => void;
  /** Edit only the given mode's object. Never touches another mode. */
  updateMode: <M extends MirrorMode>(
    mode: M,
    patch: Partial<M extends "camera" ? CameraModeSettings : ModeSettings>,
  ) => void;
  setControlConfig: (c: ControlConfig) => void;
  setAppPrefs: (p: AppPrefs) => void;
  setError: (e: string | null) => void;
  setDeviceToggle: (serial: string, key: "screenOff" | "asleep" | "swipeScroll", val: boolean) => void;
}

export const useAppStore = create<AppStore>((set) => ({
  devices: [],
  sessions: [],
  savedDevices: [],
  modeDefaults: defaultModeDefaults(),
  modeDefaultsNotice: false,
  sessionModes: {},
  controlConfig: DEFAULT_CONTROL_CONFIG,
  appPrefs: DEFAULT_APP_PREFS,
  error: null,
  deviceToggles: {},
  nicknames: {},
  selectedSerial: null,
  currentMode: "flex",
  deviceDetailsCache: {},
  batteryMap: {},
  doctorOpen: false,
  doctorSerial: null,
  timelineOpen: false,
  repairNeeded: {},
  reconnecting: {},

  setReconnecting: (key, on) =>
    set((st) => {
      if (Boolean(st.reconnecting[key]) === on) return st;
      const next = { ...st.reconnecting };
      if (on) next[key] = true;
      else delete next[key];
      return { reconnecting: next };
    }),

  setRepairNeeded: (key, msg) =>
    set((st) => {
      const next = { ...st.repairNeeded };
      if (msg) {
        next[key] = msg;
      } else {
        delete next[key];
      }
      return { repairNeeded: next };
    }),
  setNicknames: (nicknames) => set({ nicknames }),
  setSelectedSerial: (selectedSerial) => set({ selectedSerial }),
  setCurrentMode: (currentMode) => set({ currentMode }),
  setDeviceDetails: (serial, details) =>
    set((st) => ({
      deviceDetailsCache: { ...st.deviceDetailsCache, [serial]: details },
    })),
  setBatteryInfo: (serial, info) =>
    set((st) => ({
      batteryMap: { ...st.batteryMap, [serial]: info },
    })),
  openDoctor: (serial = null) => set({ doctorOpen: true, doctorSerial: serial ?? null }),
  closeDoctor: () => set({ doctorOpen: false }),
  setTimelineOpen: (timelineOpen) => set({ timelineOpen }),
  setNickname: (serial, name) =>
    set((st) => {
      const next = { ...st.nicknames };
      const trimmed = name.trim();
      if (trimmed) {
        next[serial] = trimmed;
      } else {
        delete next[serial];
      }
      return { nicknames: next };
    }),

  setDevices: (devices) => set({ devices }),
  setSessions: (sessions) => set({ sessions }),
  upsertSession: (s) =>
    set((st) => {
      const known = st.sessions.some((x) => x.id === s.id);
      return {
        sessions: known
          ? st.sessions.map((x) => (x.id === s.id ? s : x))
          : [...st.sessions, s],
        // Remember the selected mode at launch so later mode switches don't change it.
        sessionModes:
          known || st.sessionModes[s.id]
            ? st.sessionModes
            : {
                ...st.sessionModes,
                [s.id]: s.mode === "camera" ? "camera" : st.currentMode === "camera" ? "mirror" : st.currentMode,
              },
      };
    }),
  removeSession: (id) =>
    set((st) => {
      const { [id]: _gone, ...rest } = st.sessionModes;
      return { sessions: st.sessions.filter((x) => x.id !== id), sessionModes: rest };
    }),
  setSavedDevices: (savedDevices) => set({ savedDevices }),
  setModeDefaults: (modeDefaults, notice) =>
    set((st) => ({
      modeDefaults,
      modeDefaultsNotice: notice ?? st.modeDefaultsNotice,
    })),
  setModeDefaultsNotice: (modeDefaultsNotice) => set({ modeDefaultsNotice }),
  updateMode: (mode, patch) =>
    set((st) => ({
      modeDefaults: {
        ...st.modeDefaults,
        [mode]: { ...st.modeDefaults[mode], ...patch },
      },
    })),
  setControlConfig: (controlConfig) => set({ controlConfig }),
  setAppPrefs: (appPrefs) => set({ appPrefs }),
  setError: (error) => set({ error }),
  setDeviceToggle: (serial, key, val) =>
    set((st) => ({
      deviceToggles: {
        ...st.deviceToggles,
        [serial]: {
          ...st.deviceToggles[serial],
          [key]: val,
        },
      },
    })),
}));
