import { useMemo, useState } from "react";
import { BellOff, Check, Copy, RefreshCw } from "lucide-react";
import type { PhoneNotification } from "@/lib/types";

interface NotificationsViewProps {
  notifications: PhoneNotification[];
  loading: boolean;
  error: string | null;
  onRefresh: () => void;
}

function formatTime(unix: number): string {
  if (!unix) return "";
  return new Date(unix * 1000).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function NotificationsView({
  notifications,
  loading,
  error,
  onRefresh,
}: NotificationsViewProps) {
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  // Group by app (preserving newest-first group order and newest-first items within each group)
  const groups = useMemo(() => {
    const sorted = [...notifications].sort((a, b) => b.posted_unix - a.posted_unix);
    const map = new Map<string, { label: string; pkg: string; items: PhoneNotification[] }>();
    for (const n of sorted) {
      const existing = map.get(n.package);
      if (existing) {
        existing.items.push(n);
      } else {
        map.set(n.package, {
          label: n.app_label || n.package,
          pkg: n.package,
          items: [n],
        });
      }
    }
    return Array.from(map.values());
  }, [notifications]);

  async function handleCopy(n: PhoneNotification) {
    const content = [n.title, n.text].filter(Boolean).join("\n");
    try {
      await navigator.clipboard.writeText(content);
      setCopiedKey(n.key);
      setTimeout(() => setCopiedKey((prev) => (prev === n.key ? null : prev)), 1500);
    } catch {
      // Clipboard write failed
    }
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink-700 bg-ink-900 px-4 py-2">
        <span className="text-xs text-text-muted">
          Read-only — replies need the phone
        </span>
        <button
          type="button"
          disabled={loading}
          onClick={onRefresh}
          className="btn-tactile flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          Refresh
        </button>
      </div>

      {error && (
        <div className="border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-4">
        {groups.length === 0 ? (
          <div className="flex h-56 flex-col items-center justify-center text-center text-xs text-text-muted">
            <BellOff className="mb-2 h-8 w-8 text-ink-700" />
            <p className="font-medium text-text">No active notifications</p>
            <p className="mt-1 text-[11px] text-text-muted">
              Read-only — replies need the phone
            </p>
          </div>
        ) : (
          <div className="mx-auto max-w-3xl space-y-4">
            {groups.map((group) => (
              <div
                key={group.pkg}
                className="overflow-hidden rounded-xl border border-ink-700 bg-ink-900"
              >
                <div className="flex items-center justify-between border-b border-ink-700 bg-ink-800 px-3.5 py-2">
                  <span className="text-xs font-semibold text-text-strong">{group.label}</span>
                  <span className="font-mono text-[10px] text-text-muted">
                    {group.items.length}
                  </span>
                </div>
                <div className="divide-y divide-ink-700">
                  {group.items.map((n) => (
                    <div
                      key={n.key}
                      className="flex items-start justify-between gap-3 px-3.5 py-2.5"
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="truncate text-xs font-medium text-text-strong">
                            {n.title || "(No title)"}
                          </span>
                          {n.posted_unix > 0 && (
                            <span className="shrink-0 font-mono text-[10px] text-text-muted">
                              {formatTime(n.posted_unix)}
                            </span>
                          )}
                        </div>
                        {n.text && (
                          <p className="mt-0.5 whitespace-pre-wrap break-words text-xs text-text-muted">
                            {n.text}
                          </p>
                        )}
                      </div>
                      <button
                        type="button"
                        onClick={() => void handleCopy(n)}
                        className="btn-tactile flex shrink-0 items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
                      >
                        {copiedKey === n.key ? (
                          <>
                            <Check className="h-3 w-3 text-status-success" />
                            Copied
                          </>
                        ) : (
                          <>
                            <Copy className="h-3 w-3" />
                            Copy
                          </>
                        )}
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
