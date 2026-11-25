import { create } from "zustand";
import { getSharedStore } from "@/lib/settingsStore";
import type {
  Conversation,
  PhoneHubNotification,
  PhoneOverview,
} from "@/types/phone";

const KEY_MUTED_PACKAGES = "phoneHubMutedPackages";
const KEY_SENSITIVE_PACKAGES = "phoneHubSensitivePackages";
const KEY_POLL_SECS = "phoneHubNotificationPollSecs";
const KEY_GALLERY_DOWNLOAD_DIR = "phoneHubLastGalleryDownloadDir";
const KEY_GALLERY_BLUR_THUMBS = "phoneHubBlurThumbnails";

export const DEFAULT_NOTIFICATION_POLL_SECS = 10;
export const MIN_NOTIFICATION_POLL_SECS = 5;

export function clampPollSecs(secs: number): number {
  if (!Number.isFinite(secs)) return DEFAULT_NOTIFICATION_POLL_SECS;
  return Math.max(MIN_NOTIFICATION_POLL_SECS, Math.round(secs));
}

async function persistValue<T>(key: string, value: T): Promise<void> {
  try {
    const store = await getSharedStore();
    await store.set(key, value);
    await store.save();
  } catch {
    // Ignore persistence errors in non-Tauri / test environments
  }
}

export interface PhoneHubState {
  overviewsBySerial: Record<string, PhoneOverview>;
  staleBySerial: Record<string, boolean>;
  notificationsBySerial: Record<string, PhoneHubNotification[]>;
  conversationsBySerial: Record<string, Conversation[]>;
  audioPlayingBySerial: Record<string, boolean>;
  mutedPackages: string[];
  sensitivePackages: string[];
  hiddenNotificationIds: string[];
  notificationPollSecs: number;
  lastGalleryDownloadDir: string | null;
  blurThumbnails: boolean;
  settingsLoaded: boolean;

  applyOverview: (serial: string, overview: PhoneOverview) => void;
  markSerialDisconnected: (serial: string) => void;
  applyNotifications: (serial: string, items: PhoneHubNotification[]) => void;
  applyConversations: (serial: string, conversations: Conversation[]) => void;
  applyMessagesUpdated: (serial: string, conversationKey: string, conversations?: Conversation[]) => void;
  markConversationReadLocal: (serial: string, key: string) => void;
  applyAudioState: (serial: string, playing: boolean) => void;
  setMutedPackages: (packages: string[]) => void;
  mutePackage: (pkg: string) => void;
  unmutePackage: (pkg: string) => void;
  isMuted: (pkg: string) => boolean;
  setSensitivePackages: (packages: string[]) => void;
  setPackageSensitive: (pkg: string, sensitive: boolean) => void;
  togglePackageSensitive: (pkg: string) => void;
  isSensitive: (pkg: string) => boolean;
  hideNotificationLocal: (id: string) => void;
  hideNotificationsLocal: (ids: string[]) => void;
  unhideAllLocal: () => void;
  setNotificationPollSecs: (secs: number) => void;
  setLastGalleryDownloadDir: (dir: string | null) => void;
  setBlurThumbnails: (blur: boolean) => void;
  loadPersistedSettings: () => Promise<void>;
  reset: () => void;
}

const initialDataState = {
  overviewsBySerial: {} as Record<string, PhoneOverview>,
  staleBySerial: {} as Record<string, boolean>,
  notificationsBySerial: {} as Record<string, PhoneHubNotification[]>,
  conversationsBySerial: {} as Record<string, Conversation[]>,
  audioPlayingBySerial: {} as Record<string, boolean>,
  mutedPackages: [] as string[],
  sensitivePackages: [] as string[],
  hiddenNotificationIds: [] as string[],
  notificationPollSecs: DEFAULT_NOTIFICATION_POLL_SECS,
  lastGalleryDownloadDir: null as string | null,
  blurThumbnails: false,
  settingsLoaded: false,
};

export const usePhoneHubStore = create<PhoneHubState>((set, get) => ({
  ...initialDataState,

  applyOverview: (serial, overview) =>
    set((s) => ({
      overviewsBySerial: { ...s.overviewsBySerial, [serial]: overview },
      staleBySerial: { ...s.staleBySerial, [serial]: false },
    })),

  markSerialDisconnected: (serial) =>
    set((s) => {
      const nextConvs = { ...s.conversationsBySerial };
      delete nextConvs[serial];
      const nextNotifs = { ...s.notificationsBySerial };
      delete nextNotifs[serial];
      const nextAudio = { ...s.audioPlayingBySerial };
      delete nextAudio[serial];
      return {
        staleBySerial: s.overviewsBySerial[serial]
          ? { ...s.staleBySerial, [serial]: true }
          : s.staleBySerial,
        conversationsBySerial: nextConvs,
        notificationsBySerial: nextNotifs,
        audioPlayingBySerial: nextAudio,
      };
    }),

  applyNotifications: (serial, items) =>
    set((s) => ({
      notificationsBySerial: { ...s.notificationsBySerial, [serial]: items },
    })),

  applyConversations: (serial, conversations) =>
    set((s) => ({
      conversationsBySerial: {
        ...s.conversationsBySerial,
        [serial]: sortConversationsNewestFirst(conversations),
      },
    })),

  applyMessagesUpdated: (serial, _conversationKey, conversations) => {
    if (!conversations) return;
    set((s) => ({
      conversationsBySerial: {
        ...s.conversationsBySerial,
        [serial]: sortConversationsNewestFirst(conversations),
      },
    }));
  },

  markConversationReadLocal: (serial, key) =>
    set((s) => {
      const existing = s.conversationsBySerial[serial];
      if (!existing) return s;
      return {
        conversationsBySerial: {
          ...s.conversationsBySerial,
          [serial]: existing.map((c) => (c.key === key ? { ...c, unread: 0 } : c)),
        },
      };
    }),

  applyAudioState: (serial, playing) =>
    set((s) => ({
      audioPlayingBySerial: { ...s.audioPlayingBySerial, [serial]: playing },
    })),

  setMutedPackages: (packages) => {
    const deduped = Array.from(new Set(packages.filter(Boolean)));
    set({ mutedPackages: deduped });
    void persistValue(KEY_MUTED_PACKAGES, deduped);
  },

  mutePackage: (pkg) => {
    if (!pkg) return;
    const current = get().mutedPackages;
    if (current.includes(pkg)) return;
    const next = [...current, pkg];
    set({ mutedPackages: next });
    void persistValue(KEY_MUTED_PACKAGES, next);
  },

  unmutePackage: (pkg) => {
    const next = get().mutedPackages.filter((p) => p !== pkg);
    set({ mutedPackages: next });
    void persistValue(KEY_MUTED_PACKAGES, next);
  },

  isMuted: (pkg) => get().mutedPackages.includes(pkg),

  setSensitivePackages: (packages) => {
    const deduped = Array.from(new Set(packages.filter(Boolean)));
    set({ sensitivePackages: deduped });
    void persistValue(KEY_SENSITIVE_PACKAGES, deduped);
  },

  setPackageSensitive: (pkg, sensitive) => {
    if (!pkg) return;
    const current = get().sensitivePackages;
    const exists = current.includes(pkg);
    if (sensitive === exists) return;
    const next = sensitive ? [...current, pkg] : current.filter((p) => p !== pkg);
    set({ sensitivePackages: next });
    void persistValue(KEY_SENSITIVE_PACKAGES, next);
  },

  togglePackageSensitive: (pkg) => {
    if (!pkg) return;
    const current = get().sensitivePackages;
    const next = current.includes(pkg)
      ? current.filter((p) => p !== pkg)
      : [...current, pkg];
    set({ sensitivePackages: next });
    void persistValue(KEY_SENSITIVE_PACKAGES, next);
  },

  isSensitive: (pkg) => get().sensitivePackages.includes(pkg),

  hideNotificationLocal: (id) => {
    if (!id) return;
    const current = get().hiddenNotificationIds;
    if (current.includes(id)) return;
    set({ hiddenNotificationIds: [...current, id] });
  },

  hideNotificationsLocal: (ids) => {
    const filtered = ids.filter(Boolean);
    if (filtered.length === 0) return;
    const next = Array.from(new Set([...get().hiddenNotificationIds, ...filtered]));
    set({ hiddenNotificationIds: next });
  },

  unhideAllLocal: () => set({ hiddenNotificationIds: [] }),

  setNotificationPollSecs: (secs) => {
    const clamped = clampPollSecs(secs);
    set({ notificationPollSecs: clamped });
    void persistValue(KEY_POLL_SECS, clamped);
  },

  setLastGalleryDownloadDir: (dir) => {
    set({ lastGalleryDownloadDir: dir });
    void persistValue(KEY_GALLERY_DOWNLOAD_DIR, dir);
  },

  setBlurThumbnails: (blur) => {
    set({ blurThumbnails: blur });
    void persistValue(KEY_GALLERY_BLUR_THUMBS, blur);
  },

  loadPersistedSettings: async () => {
    try {
      const store = await getSharedStore();
      const muted = await store.get<string[]>(KEY_MUTED_PACKAGES);
      const sensitive = await store.get<string[]>(KEY_SENSITIVE_PACKAGES);
      const pollSecs = await store.get<number>(KEY_POLL_SECS);
      const downloadDir = await store.get<string | null>(KEY_GALLERY_DOWNLOAD_DIR);
      const blur = await store.get<boolean>(KEY_GALLERY_BLUR_THUMBS);
      set({
        mutedPackages: Array.isArray(muted) ? Array.from(new Set(muted.filter(Boolean))) : [],
        sensitivePackages: Array.isArray(sensitive)
          ? Array.from(new Set(sensitive.filter(Boolean)))
          : [],
        notificationPollSecs:
          typeof pollSecs === "number" ? clampPollSecs(pollSecs) : DEFAULT_NOTIFICATION_POLL_SECS,
        lastGalleryDownloadDir: typeof downloadDir === "string" && downloadDir ? downloadDir : null,
        blurThumbnails: typeof blur === "boolean" ? blur : false,
        settingsLoaded: true,
      });
    } catch {
      set({ settingsLoaded: true });
    }
  },

  reset: () => set({ ...initialDataState }),
}));

export function sortConversationsNewestFirst(conversations: Conversation[]): Conversation[] {
  return [...conversations].sort((a, b) => {
    const aLast = a.messages[a.messages.length - 1]?.timestampMs ?? 0;
    const bLast = b.messages[b.messages.length - 1]?.timestampMs ?? 0;
    return bLast - aLast;
  });
}

export function getTotalUnreadMessages(conversations: Conversation[] | undefined): number {
  if (!conversations) return 0;
  return conversations.reduce((sum, c) => sum + (c.unread || 0), 0);
}
