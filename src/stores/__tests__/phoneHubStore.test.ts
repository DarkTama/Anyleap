import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Conversation, PhoneHubNotification, PhoneOverview } from "@/types/phone";

const storeBacking = new Map<string, unknown>();
const mockStore = {
  get: vi.fn(async (k: string) => storeBacking.get(k)),
  set: vi.fn(async (k: string, v: unknown) => {
    storeBacking.set(k, v);
  }),
  save: vi.fn(async () => {}),
};

vi.mock("@/lib/settingsStore", () => ({
  getSharedStore: vi.fn(async () => mockStore),
}));

import {
  clampPollSecs,
  getTotalUnreadMessages,
  usePhoneHubStore,
} from "../phoneHubStore";

function makeOverview(overrides: Partial<PhoneOverview> = {}): PhoneOverview {
  return {
    model: null,
    brand: null,
    androidVersion: null,
    securityPatch: null,
    battery: null,
    storage: null,
    ram: null,
    wifi: null,
    cellular: null,
    screen: null,
    uptimeSecs: null,
    updatedAtMs: 1000,
    ...overrides,
  };
}

function makeConversation(
  key: string,
  lastTs: number,
  unread = 0,
): Conversation {
  return {
    package: "app.example.chat",
    appLabel: "Chat",
    key,
    title: `Thread ${key}`,
    isGroup: false,
    messages: [{ sender: "Alex", text: "Hello", timestampMs: lastTs }],
    unread,
  };
}

describe("phoneHubStore", () => {
  beforeEach(() => {
    storeBacking.clear();
    mockStore.get.mockClear();
    mockStore.set.mockClear();
    mockStore.save.mockClear();
    usePhoneHubStore.getState().reset();
  });

  it("stores overviews per-serial with null leaves safely and marks stale on disconnect", () => {
    const store = usePhoneHubStore.getState();
    const ovA = makeOverview({ model: "Phone A", battery: { level: 78, charging: true, tempC: null, health: null } });
    const ovB = makeOverview({ model: "Phone B", wifi: { ssid: null, signalLevel: 2 } });

    store.applyOverview("serial-a", ovA);
    store.applyOverview("serial-b", ovB);
    store.applyConversations("serial-a", [makeConversation("c1", 500, 2)]);
    store.applyAudioState("serial-a", true);

    expect(usePhoneHubStore.getState().overviewsBySerial["serial-a"]).toEqual(ovA);
    expect(usePhoneHubStore.getState().overviewsBySerial["serial-b"]).toEqual(ovB);
    expect(usePhoneHubStore.getState().staleBySerial["serial-a"]).toBe(false);

    store.markSerialDisconnected("serial-a");

    const after = usePhoneHubStore.getState();
    expect(after.staleBySerial["serial-a"]).toBe(true);
    expect(after.staleBySerial["serial-b"]).toBe(false);
    expect(after.conversationsBySerial["serial-a"]).toBeUndefined();
    expect(after.audioPlayingBySerial["serial-a"]).toBeUndefined();
    // Serial B remains untouched
    expect(after.overviewsBySerial["serial-b"]).toEqual(ovB);
  });

  it("isolates notifications per serial", () => {
    const n1: PhoneHubNotification = {
      key: "k1",
      package: "app.example.chat",
      appLabel: "Chat",
      title: "Alex",
      text: "Hi",
      postedUnix: 100,
    };
    const n2: PhoneHubNotification = {
      key: "k2",
      package: "app.example.mail",
      appLabel: "Mail",
      title: "Invoice",
      text: "Ready",
      postedUnix: 200,
    };

    const store = usePhoneHubStore.getState();
    store.applyNotifications("serial-a", [n1]);
    store.applyNotifications("serial-b", [n2]);

    expect(usePhoneHubStore.getState().notificationsBySerial["serial-a"]).toEqual([n1]);
    expect(usePhoneHubStore.getState().notificationsBySerial["serial-b"]).toEqual([n2]);
  });

  it("sorts conversations newest-first and updates unread counts", () => {
    const store = usePhoneHubStore.getState();
    store.applyConversations("serial-a", [
      makeConversation("old", 100, 1),
      makeConversation("newest", 300, 3),
      makeConversation("mid", 200, 0),
    ]);

    const list = usePhoneHubStore.getState().conversationsBySerial["serial-a"];
    expect(list.map((c) => c.key)).toEqual(["newest", "mid", "old"]);
    expect(getTotalUnreadMessages(list)).toBe(4);

    store.markConversationReadLocal("serial-a", "newest");
    const updated = usePhoneHubStore.getState().conversationsBySerial["serial-a"];
    expect(updated.find((c) => c.key === "newest")?.unread).toBe(0);
    expect(getTotalUnreadMessages(updated)).toBe(1);
  });

  it("manages muted packages with deduplication", () => {
    const store = usePhoneHubStore.getState();
    store.setMutedPackages(["app.example.a", "app.example.a", ""]);
    expect(usePhoneHubStore.getState().mutedPackages).toEqual(["app.example.a"]);
    expect(usePhoneHubStore.getState().isMuted("app.example.a")).toBe(true);
    expect(usePhoneHubStore.getState().isMuted("app.example.b")).toBe(false);

    store.mutePackage("app.example.b");
    expect(usePhoneHubStore.getState().mutedPackages).toEqual([
      "app.example.a",
      "app.example.b",
    ]);

    store.unmutePackage("app.example.a");
    expect(usePhoneHubStore.getState().mutedPackages).toEqual(["app.example.b"]);
  });

  it("clamps notificationPollSecs to minimum 5", () => {
    expect(clampPollSecs(2)).toBe(5);
    expect(clampPollSecs(15.6)).toBe(16);
    expect(clampPollSecs(Number.NaN)).toBe(10);

    usePhoneHubStore.getState().setNotificationPollSecs(1);
    expect(usePhoneHubStore.getState().notificationPollSecs).toBe(5);
  });

  it("defaults blurThumbnails to false and persists changes via shared store", async () => {
    expect(usePhoneHubStore.getState().blurThumbnails).toBe(false);

    usePhoneHubStore.getState().setBlurThumbnails(true);
    expect(usePhoneHubStore.getState().blurThumbnails).toBe(true);
    await Promise.resolve();
    expect(mockStore.set).toHaveBeenCalledWith("phoneHubBlurThumbnails", true);

    usePhoneHubStore.getState().reset();
    expect(usePhoneHubStore.getState().blurThumbnails).toBe(false);

    await usePhoneHubStore.getState().loadPersistedSettings();
    expect(usePhoneHubStore.getState().blurThumbnails).toBe(true);
  });
});
