import { create } from "zustand";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  fetchReleasesList,
  calculateVersionDistance,
  getCurrentAppVersion,
  type ReleaseItem,
} from "../services/updateChecker";

export type InstallPhase = "idle" | "checking" | "downloading" | "ready" | "error";

export interface AppUpdateState {
  phase: InstallPhase;
  progress: number | null; // 0..1 or null if content length unknown
  error: string | null;
  hasUpdate: boolean;
  currentVersion: string;
  availableVersion: string | null;
  releaseUrl: string | null;
  releases: ReleaseItem[];
  uninstalledReleases: ReleaseItem[];
  behindCount: number;
  dismissed: boolean;
  isModalOpen: boolean;

  checkForUpdates: (manual?: boolean) => Promise<void>;
  installUpdate: () => Promise<void>;
  restartApp: () => Promise<void>;
  openModal: () => void;
  closeModal: () => void;
  dismissUpdate: () => void;
  reset: () => void;
}

let pendingTauriUpdate: Update | null = null;

export const useAppUpdateStore = create<AppUpdateState>((set, get) => ({
  phase: "idle",
  progress: null,
  error: null,
  hasUpdate: false,
  currentVersion: "0.9.6",
  availableVersion: null,
  releaseUrl: null,
  releases: [],
  uninstalledReleases: [],
  behindCount: 0,
  dismissed: false,
  isModalOpen: false,

  checkForUpdates: async (manual = false) => {
    set({ phase: "checking", error: null });
    try {
      const currentVer = await getCurrentAppVersion();
      const releases = await fetchReleasesList(manual);
      const distance = calculateVersionDistance(releases, currentVer);

      const hasUpdate = !distance.isLatest;
      const latestItem = distance.uninstalledReleases[0];

      set({
        phase: "idle",
        currentVersion: currentVer,
        releases,
        hasUpdate,
        availableVersion: hasUpdate ? distance.latestVersion : null,
        releaseUrl: latestItem ? latestItem.htmlUrl : null,
        uninstalledReleases: distance.uninstalledReleases,
        behindCount: distance.behindCount,
        dismissed: false,
      });

      if (manual) {
        set({ isModalOpen: true });
      }
    } catch (err) {
      set({
        phase: "error",
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  installUpdate: async () => {
    const { phase } = get();
    if (phase === "downloading" || phase === "ready") return;

    set({ phase: "downloading", progress: 0, error: null });

    try {
      if (!pendingTauriUpdate) {
        pendingTauriUpdate = await check();
      }

      if (!pendingTauriUpdate) {
        set({
          phase: "error",
          error: "No installable update asset found on release feed yet. Try again shortly.",
        });
        return;
      }

      let total = 0;
      let received = 0;

      await pendingTauriUpdate.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
          set({ progress: total ? 0 : null });
        } else if (event.event === "Progress") {
          received += event.data.chunkLength;
          if (total > 0) {
            set({ progress: Math.min(received / total, 1) });
          }
        }
      });

      set({ phase: "ready", progress: 1 });
    } catch (err) {
      set({
        phase: "error",
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  restartApp: async () => {
    try {
      await relaunch();
    } catch (err) {
      set({
        phase: "error",
        error: `Restart failed: ${err instanceof Error ? err.message : String(err)}`,
      });
    }
  },

  openModal: () => set({ isModalOpen: true }),
  closeModal: () => set({ isModalOpen: false }),
  dismissUpdate: () => set({ dismissed: true }),

  reset: () =>
    set({
      phase: "idle",
      progress: null,
      error: null,
      hasUpdate: false,
      availableVersion: null,
      releaseUrl: null,
      releases: [],
      uninstalledReleases: [],
      behindCount: 0,
      dismissed: false,
      isModalOpen: false,
    }),
}));
