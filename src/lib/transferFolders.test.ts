import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-store", () => ({ load: vi.fn() }));

import {
  DEFAULT_PHONE_UPLOAD_DIR,
  DEFAULT_TRANSFER_FOLDERS,
  PHONE_UPLOAD_DIR_ERROR,
  normalizePhoneUploadDir,
  normalizeTransferFolders,
  validatePhoneUploadDir,
} from "./transferFolders";
import { loadTransferFoldersFrom, type KeyValueStore } from "./settingsStore";

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

describe("transferFolders (SPEC-31)", () => {
  it("provides safe defaults", () => {
    expect(DEFAULT_TRANSFER_FOLDERS).toEqual({
      receiveDir: null,
      askWhereToSave: false,
      phoneUploadDir: DEFAULT_PHONE_UPLOAD_DIR,
    });
  });

  it("normalizes missing, partial, or corrupt objects", () => {
    expect(normalizeTransferFolders(null)).toEqual(DEFAULT_TRANSFER_FOLDERS);
    expect(normalizeTransferFolders("corrupt")).toEqual(DEFAULT_TRANSFER_FOLDERS);
    expect(
      normalizeTransferFolders({
        receiveDir: "  D:\\Drops\\AnyLeap  ",
        askWhereToSave: true,
        phoneUploadDir: "/sdcard/Documents/Work/",
      }),
    ).toEqual({
      receiveDir: "D:\\Drops\\AnyLeap",
      askWhereToSave: true,
      phoneUploadDir: "/sdcard/Documents/Work",
    });
    expect(
      normalizeTransferFolders({
        receiveDir: "   ",
        askWhereToSave: "yes",
        phoneUploadDir: "/data/local/tmp",
      }),
    ).toEqual({
      receiveDir: null,
      askWhereToSave: false,
      phoneUploadDir: DEFAULT_PHONE_UPLOAD_DIR,
    });
  });

  it("validates phone upload paths identically to backend rules", () => {
    expect(normalizePhoneUploadDir("/sdcard/Download")).toEqual({
      ok: true,
      path: "/sdcard/Download",
    });
    expect(normalizePhoneUploadDir(" /storage/emulated/0/Pictures/ ")).toEqual({
      ok: true,
      path: "/storage/emulated/0/Pictures",
    });
    expect(validatePhoneUploadDir("/sdcard/Download")).toBeNull();

    for (const bad of [
      "",
      "   ",
      "sdcard/Download",
      "/sdcard/../Download",
      "/data/local/tmp",
      "/sdcard/Download\n",
      '/sdcard/"bad"',
      "/sdcard/Download;rm",
      "/sdcard/$HOME",
    ]) {
      expect(validatePhoneUploadDir(bad)).toBe(PHONE_UPLOAD_DIR_ERROR);
    }
  });

  it("loadTransferFoldersFrom writes defaults once and preserves phoneHubLastGalleryDownloadDir", async () => {
    const { store, data, calls } = fakeStore({
      phoneHubLastGalleryDownloadDir: "C:\\OldGalleryPick",
    });

    const first = await loadTransferFoldersFrom(store);
    expect(first).toEqual(DEFAULT_TRANSFER_FOLDERS);
    expect(data.transferFolders).toEqual(DEFAULT_TRANSFER_FOLDERS);
    expect(data.phoneHubLastGalleryDownloadDir).toBe("C:\\OldGalleryPick");
    expect(calls).toEqual(["set:transferFolders", "save"]);

    calls.length = 0;
    data.transferFolders = {
      receiveDir: "D:\\MyTransfers",
      askWhereToSave: true,
      phoneUploadDir: "/sdcard/Documents",
    };

    const second = await loadTransferFoldersFrom(store);
    expect(second).toEqual({
      receiveDir: "D:\\MyTransfers",
      askWhereToSave: true,
      phoneUploadDir: "/sdcard/Documents",
    });
    expect(calls).toEqual([]);
    expect(data.phoneHubLastGalleryDownloadDir).toBe("C:\\OldGalleryPick");
  });
});
