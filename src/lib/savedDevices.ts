import { load, type Store } from "@tauri-apps/plugin-store";
import type { SavedDevice } from "./types";

const FILE = "wireless.json";
const KEY = "savedDevices";

let storePromise: Promise<Store> | null = null;
const getStore = () => (storePromise ??= load(FILE, { autoSave: true, defaults: {} }));

export async function listSaved(): Promise<SavedDevice[]> {
  const store = await getStore();
  return (await store.get<SavedDevice[]>(KEY)) ?? [];
}

export async function upsertSaved(device: SavedDevice): Promise<SavedDevice[]> {
  const store = await getStore();
  const current = (await store.get<SavedDevice[]>(KEY)) ?? [];
  const next = current.some((d) => d.id === device.id)
    ? current.map((d) => (d.id === device.id ? device : d))
    : [...current, device];
  await store.set(KEY, next);
  await store.save();
  return next;
}

export async function forgetSaved(id: string): Promise<SavedDevice[]> {
  const store = await getStore();
  const next = ((await store.get<SavedDevice[]>(KEY)) ?? []).filter((d) => d.id !== id);
  await store.set(KEY, next);
  await store.save();
  return next;
}
