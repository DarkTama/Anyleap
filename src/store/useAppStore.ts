import { create } from "zustand";
import type {
  BatteryInfo,
  CoreSettings,
  DeviceDetails,
  DeviceInfo,
  QualityPreset,
  SavedDevice,
  SessionInfo,
} from "@/lib/types";
import { DEFAULT_SETTINGS } from "@/lib/types";
import type { ControlConfig } from "@/lib/controlConfig";
import { DEFAULT_CONTROL_CONFIG } from "@/lib/controlConfig";
import type { AppPrefs } from "@/lib/persist";
import { DEFAULT_APP_PREFS } from "@/lib/persist";
import type { StudioMode } from "@/components/deck/LaunchMatrix";

interface AppStore {
  devices: DeviceInfo[];
  sessions: SessionInfo[];
  savedDevices: SavedDevice[];
  settings: CoreSettings;
  preset: QualityPreset;
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

  setDevices: (d: DeviceInfo[]) => void;
  setSessions: (s: SessionInfo[]) => void;
  upsertSession: (s: SessionInfo) => void;
  removeSession: (id: string) => void;
  setSavedDevices: (d: SavedDevice[]) => void;
  setSettings: (s: CoreSettings) => void;
  setPreset: (p: QualityPreset) => void;
  setControlConfig: (c: ControlConfig) => void;
  setAppPrefs: (p: AppPrefs) => void;
  setError: (e: string | null) => void;
  setDeviceToggle: (serial: string, key: "screenOff" | "asleep" | "swipeScroll", val: boolean) => void;
}

export const useAppStore = create<AppStore>((set) => ({
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
  selectedSerial: null,
  currentMode: "flex",
  deviceDetailsCache: {},
  batteryMap: {},
  doctorOpen: false,
  doctorSerial: null,
  timelineOpen: false,

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
    set((st) => ({
      sessions: st.sessions.some((x) => x.id === s.id)
        ? st.sessions.map((x) => (x.id === s.id ? s : x))
        : [...st.sessions, s],
    })),
  removeSession: (id) =>
    set((st) => ({ sessions: st.sessions.filter((x) => x.id !== id) })),
  setSavedDevices: (savedDevices) => set({ savedDevices }),
  setSettings: (settings) => set({ settings }),
  setPreset: (preset) => set({ preset }),
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
