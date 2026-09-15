# Implementation Plan: Shared Settings Persistence (`tauri-plugin-store`)

> **Goal:** Consolidate frontend persistence across AnyLeap (currently split across `config.json` and `wireless.json`) into a single, strongly-typed store abstraction backed by `@tauri-apps/plugin-store` and `tauri-plugin-store`.

**Architecture:**
- Create a unified persistence module `src/lib/settingsStore.ts` that manages a single store file `settings.json`.
- Define unified TypeScript interfaces and schemas for all persisted states: core settings, quality presets, app preferences, control bar configurations, and saved wireless devices.
- Provide a migration path from legacy stores (`config.json`, `wireless.json`) to `settings.json` upon first load.
- Expose typed async getters and setters (`getSettings`, `setSettings`, `getSavedDevices`, `saveDevice`, etc.) used by Zustand store (`useAppStore.ts`) and UI components.

**Tech Stack:** TypeScript, `@tauri-apps/plugin-store`, `tauri-plugin-store` v2, Zustand.

---

## Global Constraints

- Preserve default values from `DEFAULT_SETTINGS`, `DEFAULT_APP_PREFS`, and `DEFAULT_CONTROL_CONFIG`.
- Do not lose previously saved wireless devices or user configurations during migration.
- Maintain atomic auto-saving without blocking React render cycles.

---

## File Changes

- Create: `src/lib/settingsStore.ts`
- Modify: `src/lib/persist.ts` (deprecate / forward to `settingsStore.ts`)
- Modify: `src/lib/controlConfig.ts` (delegate persistence to `settingsStore.ts`)
- Modify: `src/lib/savedDevices.ts` (delegate persistence to `settingsStore.ts`)
- Modify: `src/store/useAppStore.ts`

---

## Detailed Tasks

### Task 1: Create Unified `settingsStore.ts`

**Files:**
- Create: `src/lib/settingsStore.ts`

**Implementation Outline:**
```typescript
import { load, Store } from "@tauri-apps/plugin-store";
import type { CoreSettings, QualityPreset, SavedDevice } from "./types";
import type { AppPrefs } from "./persist";
import type { ControlConfig } from "./controlConfig";

const STORE_PATH = "settings.json";

let storeInstance: Promise<Store> | null = null;

export function getSharedStore(): Promise<Store> {
  if (!storeInstance) {
    storeInstance = load(STORE_PATH, { autoSave: true, defaults: {} });
  }
  return storeInstance;
}

export async function migrateLegacyStores(): Promise<void> {
  const store = await getSharedStore();
  // Migrate from config.json if unmigrated
  try {
    const oldConfig = await load("config.json", { autoSave: false });
    const settings = await oldConfig.get("settings");
    if (settings && !(await store.get("settings"))) {
      await store.set("settings", settings);
    }
  } catch { /* ignore */ }

  // Migrate wireless.json if unmigrated
  try {
    const oldWireless = await load("wireless.json", { autoSave: false });
    const saved = await oldWireless.get("savedDevices");
    if (saved && !(await store.get("savedDevices"))) {
      await store.set("savedDevices", saved);
    }
  } catch { /* ignore */ }
  await store.save();
}
```

**Verification:**
Run: `npx tsc --noEmit`
Expected: PASS

---

### Task 2: Refactor Persistence Entrypoints

**Files:**
- Modify: `src/lib/persist.ts`
- Modify: `src/lib/controlConfig.ts`
- Modify: `src/lib/savedDevices.ts`

**Implementation:**
Route `loadQuality`, `saveQuality`, `loadAppPrefs`, `saveAppPrefs`, `loadControlConfig`, `saveControlConfig`, `listSaved`, and `upsertSaved` through `getSharedStore()`.

**Verification:**
Run: `npx tsc --noEmit && npm run build`
Expected: PASS
