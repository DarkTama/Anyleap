import { load, type Store } from "@tauri-apps/plugin-store";
import type { CoreSettings, SavedDevice } from "./types";
import {
  fileSyncMigrationPatch,
  migrateLegacy,
  normalizeModeDefaults,
  type ModeDefaultsV2,
} from "./modeSettings";
import { DEFAULT_CONTROL_CONFIG, type ControlConfig } from "./controlConfig";

export interface AppPrefs {
  minimizeToTrayOnClose: boolean;
  checkUpdates: boolean;
  desktopNotifications: boolean;
  batteryAlerts: boolean;
}

export const DEFAULT_APP_PREFS: AppPrefs = {
  minimizeToTrayOnClose: true,
  checkUpdates: true,
  desktopNotifications: false,
  batteryAlerts: true,
};

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
        const preset = await oldConfig.get<string>("preset");
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

/** The minimal store surface the mode-defaults persistence needs (eases testing). */
export interface KeyValueStore {
  get<T>(key: string): Promise<T | undefined | null>;
  set(key: string, value: unknown): Promise<void>;
  save(): Promise<void>;
}

export interface LoadedModeDefaults {
  modeDefaults: ModeDefaultsV2;
  /** One-time "audio is now on" notice is pending. */
  notice: boolean;
}

/**
 * SPEC-28 §5.4 step 7. Loads `modeDefaults`; when absent, migrates the frozen legacy
 * `settings` + `preset` keys once. Writes modeDefaults, then the notice flag, then saves.
 * Legacy keys are never deleted, so a half-written store re-runs safely.
 */
export async function loadModeDefaultsFrom(store: KeyValueStore): Promise<LoadedModeDefaults> {
  const existing = await store.get<unknown>("modeDefaults");
  if (existing) {
    const notice = (await store.get<boolean>("modeDefaultsNotice")) ?? false;
    return { modeDefaults: normalizeModeDefaults(existing), notice: notice === true };
  }
  const legacy = await store.get<Partial<CoreSettings>>("settings");
  const preset = await store.get<string>("preset");
  const { modeDefaults, audioNotice } = migrateLegacy(legacy ?? null, preset);
  await store.set("modeDefaults", modeDefaults);
  await store.set("modeDefaultsNotice", audioNotice);
  await store.save();
  return { modeDefaults, notice: audioNotice };
}

export async function loadModeDefaults(): Promise<LoadedModeDefaults> {
  return loadModeDefaultsFrom(await getStore());
}

export async function saveModeDefaults(modeDefaults: ModeDefaultsV2): Promise<void> {
  const store = await getStore();
  await store.set("modeDefaults", modeDefaults);
  await store.save();
}

export async function saveModeDefaultsNotice(pending: boolean): Promise<void> {
  const store = await getStore();
  await store.set("modeDefaultsNotice", pending);
  await store.save();
}

/**
 * SPEC-28 §5.4 step 6, once (flag `fileSyncMigrated`): when the retired file gate was
 * off, turn file copy-paste off through the normal KVM settings path.
 */
export async function runFileSyncMigrationFrom(
  store: KeyValueStore,
  kvm: {
    get: () => { syncFileTransfer?: boolean; continuityFileClipboard?: boolean };
    update: (patch: { continuityFileClipboard: false }) => void;
  },
): Promise<boolean> {
  if ((await store.get<boolean>("fileSyncMigrated")) === true) return false;
  const patch = fileSyncMigrationPatch(kvm.get());
  if (patch) kvm.update(patch);
  await store.set("fileSyncMigrated", true);
  await store.save();
  return patch !== null;
}

export async function runFileSyncMigration(kvm: Parameters<typeof runFileSyncMigrationFrom>[1]) {
  return runFileSyncMigrationFrom(await getStore(), kvm);
}

export async function loadAppPrefs(): Promise<AppPrefs> {
  const store = await getStore();
  const saved = await store.get<Partial<AppPrefs>>("appPrefs");
  return {
    minimizeToTrayOnClose:
      saved?.minimizeToTrayOnClose ?? DEFAULT_APP_PREFS.minimizeToTrayOnClose,
    checkUpdates: saved?.checkUpdates ?? DEFAULT_APP_PREFS.checkUpdates,
    desktopNotifications:
      saved?.desktopNotifications ?? DEFAULT_APP_PREFS.desktopNotifications,
    batteryAlerts: saved?.batteryAlerts ?? DEFAULT_APP_PREFS.batteryAlerts,
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

export async function replaceSavedDevices(devices: SavedDevice[]): Promise<SavedDevice[]> {
  const store = await getStore();
  await store.set("savedDevices", devices);
  await store.save();
  return devices;
}

export async function saveDevice(device: SavedDevice): Promise<SavedDevice[]> {
  const store = await getStore();
  const current = (await store.get<SavedDevice[]>("savedDevices")) ?? [];
  const matchesExisting = (d: SavedDevice) =>
    d.id === device.id ||
    (Boolean(device.hw_serial) && d.hw_serial === device.hw_serial) ||
    (Boolean(device.hw_serial) && d.id === device.host) ||
    (!d.hw_serial && Boolean(device.host) && d.host === device.host);

  const existing = current.find(matchesExisting);
  const mergedLabel =
    existing?.label &&
    existing.label !== existing.host &&
    existing.label !== existing.lastSerial &&
    existing.label !== `${existing.host}:${existing.port}`
      ? existing.label
      : device.label;
  const normalized: SavedDevice = {
    ...existing,
    ...device,
    id: device.hw_serial || device.id,
    label: mergedLabel || device.label,
  };

  const remaining = current.filter((d) => !matchesExisting(d));
  const next = [...remaining, normalized];
  await store.set("savedDevices", next);
  await store.save();
  return next;
}

export async function forgetDevice(id: string): Promise<SavedDevice[]> {
  const store = await getStore();
  const next = ((await store.get<SavedDevice[]>("savedDevices")) ?? []).filter(
    (d) => d.id !== id && d.hw_serial !== id,
  );
  await store.set("savedDevices", next);
  await store.save();
  return next;
}

export const listSaved = listSavedDevices;
export const upsertSaved = saveDevice;
export const forgetSaved = forgetDevice;
export const getSavedDevices = listSavedDevices;

export async function getNicknames(): Promise<Record<string, string>> {
  const store = await getStore();
  return (await store.get<Record<string, string>>("nicknames")) ?? {};
}

export async function replaceNicknames(
  nicknames: Record<string, string>,
): Promise<Record<string, string>> {
  const store = await getStore();
  await store.set("nicknames", nicknames);
  await store.save();
  return nicknames;
}

export async function saveNickname(
  serial: string,
  name: string,
): Promise<Record<string, string>> {
  const store = await getStore();
  const current = (await store.get<Record<string, string>>("nicknames")) ?? {};
  const trimmed = name.trim();
  const next = { ...current };
  if (trimmed) {
    next[serial] = trimmed;
  } else {
    delete next[serial];
  }
  await store.set("nicknames", next);
  await store.save();
  return next;
}

export async function loadActivities<T>(): Promise<T[]> {
  const store = await getStore();
  return (await store.get<T[]>("activities")) ?? [];
}

export async function saveActivities<T>(items: T[]): Promise<void> {
  const store = await getStore();
  await store.set("activities", items);
  await store.save();
}
