import { useEffect, useMemo, useState } from "react";
import {
  AppWindow,
  Camera,
  Clipboard,
  Clock,
  FileUp,
  Monitor,
  Network,
  Smartphone,
  Trash2,
  Video,
  X,
} from "lucide-react";
import {
  formatDuration,
  formatRelativeTime,
  useActivityStore,
  type Activity,
  type ActivityKind,
} from "@/stores/activityStore";
import { useAppStore } from "@/store/useAppStore";
import { usePresence } from "@/lib/usePresence";

const KIND_META: Record<
  ActivityKind,
  { label: string; icon: typeof Monitor; color: string }
> = {
  mirror: { label: "Mirror", icon: Monitor, color: "text-status-success" },
  camera: { label: "Camera", icon: Video, color: "text-mist-300" },
  screenshot: { label: "Screenshot", icon: Camera, color: "text-status-warning" },
  files: { label: "Files", icon: FileUp, color: "text-gold-400" },
  clipboard: { label: "Clipboard", icon: Clipboard, color: "text-mist-300" },
  app: { label: "Apps", icon: AppWindow, color: "text-gold-400" },
  kvm: { label: "KVM", icon: Network, color: "text-mist-300" },
  device: { label: "Device", icon: Smartphone, color: "text-text" },
};

const ALL_KINDS: ActivityKind[] = [
  "mirror",
  "camera",
  "screenshot",
  "files",
  "clipboard",
  "app",
  "kvm",
  "device",
];

function useNowTick(intervalMs = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}

function ActivityRow({ item, now }: { item: Activity; now: number }) {
  const meta = KIND_META[item.kind] ?? KIND_META.device;
  const Icon = meta.icon;

  return (
    <div className="flex items-start gap-2.5 rounded-lg border border-ink-700 bg-ink-900 px-2.5 py-2">
      <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-ink-700 bg-ink-800">
        <Icon className={`h-3.5 w-3.5 ${meta.color}`} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-2">
          <p className="truncate text-xs font-medium text-text-strong">{item.title}</p>
          <span className="shrink-0 font-mono text-[10px] text-text-muted">
            {formatRelativeTime(item.at, now)}
          </span>
        </div>
        {(item.detail || item.durationMs !== undefined) && (
          <p className="mt-0.5 truncate font-mono text-[10px] text-text-muted">
            {[
              item.durationMs !== undefined ? formatDuration(item.durationMs) : null,
              item.detail ?? null,
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
        )}
      </div>
    </div>
  );
}

export function TimelinePanel() {
  const activities = useActivityStore((s) => s.activities);
  const setTimelineOpen = useAppStore((s) => s.setTimelineOpen);
  const now = useNowTick();
  const latest = activities.slice(0, 5);

  return (
    <aside className="flex w-68 shrink-0 flex-col border-l border-ink-700 bg-ink-950 p-4 select-none">
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-1.5">
          <Clock className="h-3.5 w-3.5 text-text-muted" />
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
            Timeline
          </span>
        </div>
        <button
          type="button"
          onClick={() => setTimelineOpen(true)}
          className="btn-tactile font-mono text-[10px] text-text-muted hover:text-gold-400 underline cursor-pointer"
        >
          View all
        </button>
      </div>

      {latest.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center rounded-xl border border-dashed border-ink-700 px-3 py-6 text-center">
          <Clock className="mb-2 h-5 w-5 text-text-muted" />
          <p className="text-xs font-medium text-text">No activity yet</p>
          <p className="mt-1 text-[11px] text-text-muted">
            Mirror sessions, file pushes, and device events appear here.
          </p>
        </div>
      ) : (
        <div className="space-y-2 overflow-y-auto">
          {latest.map((item) => (
            <ActivityRow key={item.id} item={item} now={now} />
          ))}
        </div>
      )}
    </aside>
  );
}

export function TimelineDrawer() {
  const open = useAppStore((s) => s.timelineOpen);
  const setTimelineOpen = useAppStore((s) => s.setTimelineOpen);
  const activities = useActivityStore((s) => s.activities);
  const clear = useActivityStore((s) => s.clear);
  const [filterKind, setFilterKind] = useState<ActivityKind | "all">("all");
  const now = useNowTick();

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setTimelineOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, setTimelineOpen]);

  const filtered = useMemo(() => {
    if (filterKind === "all") return activities;
    return activities.filter((a) => a.kind === filterKind);
  }, [activities, filterKind]);

  const { mounted, closing } = usePresence(open, 150);
  if (!mounted) return null;

  const state = closing ? "closing" : "open";

  return (
    <div
      data-state={state}
      className="modal-backdrop fixed inset-0 z-50 flex justify-end bg-black/65 select-none"
      onClick={(e) => {
        if (e.target === e.currentTarget) setTimelineOpen(false);
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Activity timeline"
        data-state={state}
        className="drawer-panel flex h-full w-full max-w-md flex-col border-l border-ink-700 bg-ink-950 text-text shadow-elevated"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink-700 px-4 py-3">
          <div className="flex items-center gap-2">
            <Clock className="h-4 w-4 text-text-muted" />
            <h2 className="text-sm font-semibold text-text-strong">Activity Timeline</h2>
            <span className="rounded border border-ink-700 bg-ink-900 px-1.5 py-0.5 font-mono text-[10px] text-text-muted">
              {filtered.length}
            </span>
          </div>
          <div className="flex items-center gap-2">
            {activities.length > 0 && (
              <button
                type="button"
                onClick={() => void clear()}
                className="btn-tactile flex items-center gap-1 rounded-md border border-ink-700 bg-ink-900 px-2 py-1 text-[11px] font-medium text-text hover:border-status-danger/50 hover:text-status-danger cursor-pointer"
              >
                <Trash2 className="h-3 w-3" />
                <span>Clear history</span>
              </button>
            )}
            <button
              type="button"
              onClick={() => setTimelineOpen(false)}
              className="btn-tactile rounded-md p-1 text-text-muted hover:bg-ink-800 hover:text-text-strong cursor-pointer"
              aria-label="Close timeline"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        {/* Filter Chips */}
        <div className="flex flex-wrap gap-1.5 border-b border-ink-700 px-4 py-2.5">
          <button
            type="button"
            onClick={() => setFilterKind("all")}
            className={`btn-tactile rounded-full px-2.5 py-0.5 text-[11px] font-medium cursor-pointer ${
              filterKind === "all"
                ? "bg-gold-400 text-ink-950 font-semibold"
                : "border border-ink-700 bg-ink-900 text-text-muted hover:text-text-strong"
            }`}
          >
            All
          </button>
          {ALL_KINDS.map((kind) => {
            const active = filterKind === kind;
            return (
              <button
                key={kind}
                type="button"
                onClick={() => setFilterKind(kind)}
                className={`btn-tactile rounded-full px-2.5 py-0.5 text-[11px] font-medium cursor-pointer ${
                  active
                    ? "bg-gold-400 text-ink-950 font-semibold"
                    : "border border-ink-700 bg-ink-900 text-text-muted hover:text-text-strong"
                }`}
              >
                {KIND_META[kind].label}
              </button>
            );
          })}
        </div>

        {/* Full List */}
        <div className="flex-1 space-y-2 overflow-y-auto p-4">
          {filtered.length === 0 ? (
            <div className="flex h-40 items-center justify-center text-xs text-text-muted">
              No matching activity entries.
            </div>
          ) : (
            filtered.map((item) => <ActivityRow key={item.id} item={item} now={now} />)
          )}
        </div>
      </div>
    </div>
  );
}
