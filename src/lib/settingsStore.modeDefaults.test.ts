import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-store", () => ({ load: vi.fn() }));

import {
  loadModeDefaultsFrom,
  runFileSyncMigrationFrom,
  type KeyValueStore,
} from "./settingsStore";
import { defaultModeDefaults } from "./modeSettings";

function fakeStore(initial: Record<string, unknown>) {
  const data: Record<string, unknown> = structuredClone(initial);
  const calls: string[] = [];
  const store: KeyValueStore = {
    get: async <T,>(k: string) => data[k] as T,
    set: async (k, v) => {
      calls.push(`set:${k}`);
      data[k] = v;
    },
    save: async () => {
      calls.push("save");
    },
  };
  return { store, data, calls };
}

// Exact settings/preset keys of a real profile (noAudio on, flex on, h265, 50 Mbps, 60 fps).
const REAL = {
  preset: "custom",
  settings: {
    embedded: true,
    flexDisplay: true,
    flexDisplaySize: "1920x1200/200",
    fullscreen: false,
    hideVirtualTaskbar: false,
    maxFps: 60,
    maxSize: 0,
    newDisplay: true,
    newDisplaySize: "",
    noAudio: true,
    noControl: false,
    noKeyboardIme: true,
    noWindowAspectRatioLock: true,
    renderFit: "unscaled",
    showTouches: false,
    stayAwake: true,
    turnScreenOff: true,
    videoBitRate: 50000000,
    videoCodec: "h265",
  },
};

describe("loadModeDefaultsFrom", () => {
  it("fresh install writes defaults and no notice", async () => {
    const { store, data } = fakeStore({});
    const r = await loadModeDefaultsFrom(store);
    expect(r.modeDefaults).toEqual(defaultModeDefaults());
    expect(r.notice).toBe(false);
    expect(data.modeDefaults).toEqual(defaultModeDefaults());
  });

  it("migrates the real profile: writes modeDefaults, notice, save; legacy keys frozen", async () => {
    const { store, data, calls } = fakeStore(REAL);
    const r = await loadModeDefaultsFrom(store);
    expect(r.notice).toBe(true);
    for (const m of ["mirror", "flex", "hifi"] as const) {
      expect(r.modeDefaults[m].noAudio).toBe(false);
      expect(r.modeDefaults[m].videoBitRate).toBe(50_000_000);
    }
    expect(calls).toEqual(["set:modeDefaults", "set:modeDefaultsNotice", "save"]);
    expect(data.settings).toEqual(REAL.settings);
    expect(data.preset).toBe("custom");
  });

  it("is idempotent: a second load does not migrate again and keeps edits", async () => {
    const { store, data, calls } = fakeStore(REAL);
    const first = await loadModeDefaultsFrom(store);
    data.modeDefaults = {
      ...first.modeDefaults,
      mirror: { ...first.modeDefaults.mirror, noAudio: true },
    };
    calls.length = 0;
    const second = await loadModeDefaultsFrom(store);
    expect(second.modeDefaults.mirror.noAudio).toBe(true);
    expect(second.notice).toBe(true);
    expect(calls).toEqual([]);
  });

  it("a half-written store (no modeDefaults) re-runs safely", async () => {
    const { store, data } = fakeStore({ ...REAL, modeDefaultsNotice: true });
    const a = await loadModeDefaultsFrom(store);
    delete data.modeDefaults;
    const b = await loadModeDefaultsFrom(store);
    expect(b.modeDefaults).toEqual(a.modeDefaults);
  });
});

describe("runFileSyncMigrationFrom", () => {
  it("turns file copy-paste off once when the retired gate was off", async () => {
    const { store, data } = fakeStore({});
    const update = vi.fn();
    const kvm = {
      get: () => ({ syncFileTransfer: false, continuityFileClipboard: true }),
      update,
    };
    expect(await runFileSyncMigrationFrom(store, kvm)).toBe(true);
    expect(update).toHaveBeenCalledWith({ continuityFileClipboard: false });
    expect(data.fileSyncMigrated).toBe(true);
    expect(await runFileSyncMigrationFrom(store, kvm)).toBe(false);
    expect(update).toHaveBeenCalledTimes(1);
  });

  it("only sets the flag when nothing needs changing", async () => {
    const { store, data } = fakeStore({});
    const update = vi.fn();
    await runFileSyncMigrationFrom(store, {
      get: () => ({ syncFileTransfer: true, continuityFileClipboard: true }),
      update,
    });
    expect(update).not.toHaveBeenCalled();
    expect(data.fileSyncMigrated).toBe(true);
  });
});
