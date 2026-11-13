import { create } from "zustand";
import { loadActivities, saveActivities } from "@/lib/persist";

export type ActivityKind =
  | "mirror"
  | "camera"
  | "screenshot"
  | "files"
  | "clipboard"
  | "app"
  | "kvm"
  | "device";

export interface Activity {
  id: string;
  kind: ActivityKind;
  title: string;
  detail?: string;
  at: number;
  durationMs?: number;
}

export const MAX_ACTIVITIES = 200;

interface ActivityStore {
  activities: Activity[];
  loaded: boolean;
  load: () => Promise<void>;
  clear: () => Promise<void>;
}

function makeId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

export const useActivityStore = create<ActivityStore>((set, get) => ({
  activities: [],
  loaded: false,

  load: async () => {
    try {
      const persisted = await loadActivities<Activity>();
      const current = get().activities;
      // Merge any activities logged before hydration completed, newest first.
      const byId = new Map<string, Activity>();
      for (const item of persisted) {
        if (item && typeof item.id === "string" && typeof item.at === "number") {
          byId.set(item.id, item);
        }
      }
      for (const item of current) {
        byId.set(item.id, item);
      }
      const merged = Array.from(byId.values())
        .sort((a, b) => b.at - a.at)
        .slice(0, MAX_ACTIVITIES);
      set({ activities: merged, loaded: true });
    } catch {
      set({ loaded: true });
    }
  },

  clear: async () => {
    set({ activities: [] });
    await saveActivities<Activity>([]).catch(() => {});
  },
}));

/**
 * Single public entry point for recording an activity in the timeline.
 * Prepends the entry (newest first), caps at MAX_ACTIVITIES (200), and persists.
 */
export function logActivity(a: Omit<Activity, "id" | "at">): void {
  const entry: Activity = {
    ...a,
    id: makeId(),
    at: Date.now(),
  };
  const prev = useActivityStore.getState().activities;
  const next = [entry, ...prev].slice(0, MAX_ACTIVITIES);
  useActivityStore.setState({ activities: next });
  void saveActivities<Activity>(next).catch(() => {});
}

export function formatRelativeTime(at: number, now = Date.now()): string {
  const diffSec = Math.max(0, Math.floor((now - at) / 1000));
  if (diffSec < 60) return "just now";
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin} m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr} h ago`;
  const diffDay = Math.floor(diffHr / 24);
  return `${diffDay} d ago`;
}

export function formatDuration(durationMs: number): string {
  const totalSec = Math.max(0, Math.round(durationMs / 1000));
  if (totalSec < 60) return `${totalSec} s`;
  const min = Math.round(totalSec / 60);
  if (min < 60) return `${min} min`;
  const hr = Math.floor(min / 60);
  const remMin = min % 60;
  return remMin > 0 ? `${hr} h ${remMin} m` : `${hr} h`;
}
