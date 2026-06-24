import { load, type Store } from "@tauri-apps/plugin-store";
import type { CoreSettings, QualityPreset } from "./types";
import { DEFAULT_SETTINGS } from "./types";

// Persisted app preferences (separate from quality settings + control-bar config).
export interface AppPrefs {
  minimizeToTrayOnClose: boolean;
  checkUpdates: boolean;
}

export const DEFAULT_APP_PREFS: AppPrefs = {
  minimizeToTrayOnClose: true,
  checkUpdates: true,
};

export interface Quality {
  settings: CoreSettings;
  preset: QualityPreset;
}

// Shares config.json with controlConfig.ts — the store plugin dedups by file.
const FILE = "config.json";
let storePromise: Promise<Store> | null = null;
const getStore = () => (storePromise ??= load(FILE, { autoSave: true, defaults: {} }));

export async function loadQuality(): Promise<Quality | null> {
  const store = await getStore();
  const settings = await store.get<CoreSettings>("settings");
  const preset = await store.get<QualityPreset>("preset");
  if (!settings || !preset) return null;
  return { settings: { ...DEFAULT_SETTINGS, ...settings }, preset };
}

export async function saveQuality(settings: CoreSettings, preset: QualityPreset): Promise<void> {
  const store = await getStore();
  await store.set("settings", settings);
  await store.set("preset", preset);
  await store.save();
}

export async function loadAppPrefs(): Promise<AppPrefs> {
  const store = await getStore();
  const saved = await store.get<Partial<AppPrefs>>("appPrefs");
  return {
    minimizeToTrayOnClose:
      saved?.minimizeToTrayOnClose ?? DEFAULT_APP_PREFS.minimizeToTrayOnClose,
    checkUpdates: saved?.checkUpdates ?? DEFAULT_APP_PREFS.checkUpdates,
  };
}

export async function saveAppPrefs(prefs: AppPrefs): Promise<void> {
  const store = await getStore();
  await store.set("appPrefs", prefs);
  await store.save();
}
