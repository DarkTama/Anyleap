import { describe, it, expect, beforeEach, vi } from "vitest";

const mockStorage = new Map<string, unknown>();

vi.mock("@/lib/persist", () => ({
  loadActivities: vi.fn(async () => {
    const val = mockStorage.get("activities");
    return Array.isArray(val) ? structuredClone(val) : [];
  }),
  saveActivities: vi.fn(async (items: unknown[]) => {
    mockStorage.set("activities", structuredClone(items));
  }),
}));

import {
  logActivity,
  MAX_ACTIVITIES,
  useActivityStore,
  formatRelativeTime,
  formatDuration,
} from "../activityStore";

describe("activityStore", () => {
  beforeEach(() => {
    mockStorage.clear();
    useActivityStore.setState({ activities: [], loaded: false });
  });

  it("prepends newest activities first and caps at MAX_ACTIVITIES (200)", () => {
    for (let i = 0; i < MAX_ACTIVITIES + 15; i++) {
      logActivity({ kind: "device", title: `Event ${i}` });
    }

    const items = useActivityStore.getState().activities;
    expect(items).toHaveLength(MAX_ACTIVITIES);
    expect(items[0].title).toBe(`Event ${MAX_ACTIVITIES + 14}`);
    expect(items[MAX_ACTIVITIES - 1].title).toBe("Event 15");
  });

  it("persists and restores activities across load()", async () => {
    logActivity({ kind: "mirror", title: "Mirrored TECNO CM7", durationMs: 720_000 });
    logActivity({ kind: "files", title: "Sent 3 files", detail: "192.168.1.10:5555" });

    // Wait a microtask for fire-and-forget saveActivities
    await Promise.resolve();

    // Reset in-memory store and reload from mocked persistence
    useActivityStore.setState({ activities: [], loaded: false });
    await useActivityStore.getState().load();

    const restored = useActivityStore.getState().activities;
    expect(restored).toHaveLength(2);
    expect(restored[0].title).toBe("Sent 3 files");
    expect(restored[1].title).toBe("Mirrored TECNO CM7");
    expect(restored[1].durationMs).toBe(720_000);
  });

  it("clears history in memory and persistence", async () => {
    logActivity({ kind: "screenshot", title: "Screenshot taken" });
    await Promise.resolve();
    await useActivityStore.getState().clear();

    expect(useActivityStore.getState().activities).toHaveLength(0);
    expect(mockStorage.get("activities")).toEqual([]);
  });

  it("formats relative time and duration", () => {
    const now = 1_700_000_000_000;
    expect(formatRelativeTime(now - 10_000, now)).toBe("just now");
    expect(formatRelativeTime(now - 120_000, now)).toBe("2 m ago");
    expect(formatRelativeTime(now - 7_200_000, now)).toBe("2 h ago");
    expect(formatDuration(45_000)).toBe("45 s");
    expect(formatDuration(720_000)).toBe("12 min");
  });
});
