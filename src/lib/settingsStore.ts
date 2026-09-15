import { load, type Store } from "@tauri-apps/plugin-store";
import type { CoreSettings, QualityPreset, SavedDevice } from "./types";
import { DEFAULT_SETTINGS } from "./types";
import { DEFAULT_CONTROL_CONFIG, type ControlConfig } from "./controlConfig";

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

const STORE_PATH = "settings.json";

let storeInstance: Promise<Store> | null = null;
let migrationPromise: Promise<void> | null = null;

export function getSharedStore(): Promise<Store> {
  if (!storeInstance) {
    storeInstance = load(STORE_PATH, { autoSave: true, defaults: {} });
  }
  return storeInstance;
}

export async function migrateLegacyStores(): Promise<void> {
  if (migrationPromise) return migrationPromise;

  migrationPromise = (async () => {
    try {
      const store = await getSharedStore();
      const alreadyMigrated = await store.get<boolean>("_migrated_legacy");
      if (alreadyMigrated) return;

      // Migrate from config.json if unmigrated
      try {
        const oldConfig = await load("config.json", { autoSave: false, defaults: {} });
        const settings = await oldConfig.get<CoreSettings>("settings");
        if (settings && !(await store.get("settings"))) {
          await store.set("settings", settings);
        }
        const preset = await oldConfig.get<QualityPreset>("preset");
        if (preset && !(await store.get("preset"))) {
          await store.set("preset", preset);
        }
        const appPrefs = await oldConfig.get<AppPrefs>("appPrefs");
        if (appPrefs && !(await store.get("appPrefs"))) {
          await store.set("appPrefs", appPrefs);
        }
        const controlBar = await oldConfig.get<ControlConfig>("controlBar");
        if (controlBar && !(await store.get("controlBar"))) {
          await store.set("controlBar", controlBar);
        }
      } catch {
        /* ignore missing or unreadable config.json */
      }

      // Migrate from wireless.json if unmigrated
      try {
        const oldWireless = await load("wireless.json", { autoSave: false, defaults: {} });
        const saved = await oldWireless.get<SavedDevice[]>("savedDevices");
        const currentSaved = await store.get<SavedDevice[]>("savedDevices");
        if (saved && (!currentSaved || currentSaved.length === 0)) {
          await store.set("savedDevices", saved);
        }
      } catch {
        /* ignore missing or unreadable wireless.json */
      }

      await store.set("_migrated_legacy", true);
      await store.save();
    } catch {
      /* ignore migration failures */
    }
  })();

  return migrationPromise;
}

async function getStore(): Promise<Store> {
  await migrateLegacyStores();
  return getSharedStore();
}

export async function loadSettings(): Promise<Quality | null> {
  const store = await getStore();
  const settings = await store.get<CoreSettings>("settings");
  const preset = await store.get<QualityPreset>("preset");
  if (!settings || !preset) return null;
  return { settings: { ...DEFAULT_SETTINGS, ...settings }, preset };
}

export async function saveSettings(
  settings: CoreSettings,
  preset: QualityPreset = "custom",
): Promise<void> {
  const store = await getStore();
  await store.set("settings", settings);
  await store.set("preset", preset);
  await store.save();
}

export const loadQuality = loadSettings;
export const saveQuality = saveSettings;

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

export async function loadControlConfig(): Promise<ControlConfig> {
  const store = await getStore();
  const saved = await store.get<Partial<ControlConfig>>("controlBar");
  if (!saved) return DEFAULT_CONTROL_CONFIG;
  return {
    dock: saved.dock ?? DEFAULT_CONTROL_CONFIG.dock,
    size: saved.size ?? DEFAULT_CONTROL_CONFIG.size,
    buttons: { ...DEFAULT_CONTROL_CONFIG.buttons, ...(saved.buttons ?? {}) },
    collapsed: saved.collapsed ?? DEFAULT_CONTROL_CONFIG.collapsed,
  };
}

export async function saveControlConfig(config: ControlConfig): Promise<void> {
  const store = await getStore();
  await store.set("controlBar", config);
  await store.save();
}

export async function listSavedDevices(): Promise<SavedDevice[]> {
  const store = await getStore();
  return (await store.get<SavedDevice[]>("savedDevices")) ?? [];
}

export async function saveDevice(device: SavedDevice): Promise<SavedDevice[]> {
  const store = await getStore();
  const current = (await store.get<SavedDevice[]>("savedDevices")) ?? [];
  const next = current.some((d) => d.id === device.id)
    ? current.map((d) => (d.id === device.id ? device : d))
    : [...current, device];
  await store.set("savedDevices", next);
  await store.save();
  return next;
}

export async function forgetDevice(id: string): Promise<SavedDevice[]> {
  const store = await getStore();
  const next = ((await store.get<SavedDevice[]>("savedDevices")) ?? []).filter(
    (d) => d.id !== id,
  );
  await store.set("savedDevices", next);
  await store.save();
  return next;
}

export const listSaved = listSavedDevices;
export const upsertSaved = saveDevice;
export const forgetSaved = forgetDevice;
export const getSettings = loadSettings;
export const setSettings = saveSettings;
export const getSavedDevices = listSavedDevices;
