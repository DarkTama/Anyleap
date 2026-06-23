import { create } from "zustand";
import type {
  CoreSettings,
  DeviceInfo,
  QualityPreset,
  SavedDevice,
  SessionInfo,
} from "@/lib/types";
import { DEFAULT_SETTINGS } from "@/lib/types";

interface AppStore {
  devices: DeviceInfo[];
  sessions: SessionInfo[];
  savedDevices: SavedDevice[];
  settings: CoreSettings;
  preset: QualityPreset;
  error: string | null;

  setDevices: (d: DeviceInfo[]) => void;
  setSessions: (s: SessionInfo[]) => void;
  upsertSession: (s: SessionInfo) => void;
  removeSession: (id: string) => void;
  setSavedDevices: (d: SavedDevice[]) => void;
  setSettings: (s: CoreSettings) => void;
  setPreset: (p: QualityPreset) => void;
  setError: (e: string | null) => void;
}

export const useAppStore = create<AppStore>((set) => ({
  devices: [],
  sessions: [],
  savedDevices: [],
  settings: DEFAULT_SETTINGS,
  preset: "medium",
  error: null,

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
  setError: (error) => set({ error }),
}));
