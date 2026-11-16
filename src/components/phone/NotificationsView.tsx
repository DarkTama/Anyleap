import { useEffect, useMemo, useRef, useState } from "react";
import {
  BellOff,
  Check,
  ChevronDown,
  Copy,
  ExternalLink,
  Info,
  RefreshCw,
} from "lucide-react";
import { phoneOpenApp } from "@/services/phoneHubService";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import { useAppStore } from "@/store/useAppStore";
import { saveAppPrefs } from "@/lib/persist";
import { logActivity } from "@/stores/activityStore";
import type { PhoneHubNotification } from "@/types/phone";

interface NotificationsViewProps {
  serial: string;
  notifications: PhoneHubNotification[];
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
  serial,
  notifications,
  loading,
  error,
  onRefresh,
}: NotificationsViewProps) {
  const mutedPackages = usePhoneHubStore((s) => s.mutedPackages);
  const mutePackage = usePhoneHubStore((s) => s.mutePackage);
  const unmutePackage = usePhoneHubStore((s) => s.unmutePackage);

  const appPrefs = useAppStore((s) => s.appPrefs);
  const setAppPrefs = useAppStore((s) => s.setAppPrefs);

  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [openMenuPkg, setOpenMenuPkg] = useState<string | null>(null);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const triggerRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const menuRef = useRef<HTMLDivElement | null>(null);

  const mutedSet = useMemo(() => new Set(mutedPackages), [mutedPackages]);

  // Remember app labels seen in notifications for muted summary display
  const labelByPkg = useMemo(() => {
    const map = new Map<string, string>();
    for (const n of notifications) {
      if (n.appLabel && n.appLabel !== n.package) {
        map.set(n.package, n.appLabel);
      }
    }
    return map;
  }, [notifications]);

  const hiddenCount = useMemo(
    () => notifications.filter((n) => mutedSet.has(n.package)).length,
    [notifications, mutedSet],
  );

  const groups = useMemo(() => {
    const sorted = [...notifications]
      .filter((n) => !mutedSet.has(n.package))
      .sort((a, b) => b.postedUnix - a.postedUnix);

    const map = new Map<
      string,
      { label: string; hasResolvedLabel: boolean; pkg: string; items: PhoneHubNotification[] }
    >();
    for (const n of sorted) {
      const existing = map.get(n.package);
      if (existing) {
        existing.items.push(n);
      } else {
        const resolved = Boolean(n.appLabel && n.appLabel !== n.package);
        map.set(n.package, {
          label: n.appLabel || n.package,
          hasResolvedLabel: resolved,
          pkg: n.package,
          items: [n],
        });
      }
    }
    return Array.from(map.values());
  }, [notifications, mutedSet]);

  const previewNotification = useMemo(() => {
    return (
      [...notifications]
        .filter((n) => !mutedSet.has(n.package))
        .sort((a, b) => b.postedUnix - a.postedUnix)[0] ?? null
    );
  }, [notifications, mutedSet]);

  // Close menu on outside click
  useEffect(() => {
    if (!openMenuPkg) return;
    const onDocClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setOpenMenuPkg(null);
      }
    };
    document.addEventListener("click", onDocClick);
    return () => document.removeEventListener("click", onDocClick);
  }, [openMenuPkg]);

  // Focus first item when menu opens
  useEffect(() => {
    if (!openMenuPkg || !menuRef.current) return;
    const firstBtn = menuRef.current.querySelector<HTMLButtonElement>("button[role='menuitem']");
    firstBtn?.focus();
  }, [openMenuPkg]);

  async function handleCopy(n: PhoneHubNotification) {
    const content = [n.title, n.text].filter(Boolean).join("\n");
    try {
      await navigator.clipboard.writeText(content);
      setCopiedKey(n.key);
      setTimeout(() => setCopiedKey((prev) => (prev === n.key ? null : prev)), 1500);
    } catch {
      // Clipboard write failed
    }
  }

  async function handleOpenApp(pkg: string, label: string) {
    setOpenMenuPkg(null);
    try {
      await phoneOpenApp(serial, pkg);
      setActionMsg(`Opening ${label} on PC…`);
      logActivity({ kind: "app", title: `Opened ${label}`, detail: pkg });
    } catch (e) {
      setActionMsg(String(e));
    }
  }

  function handleMuteApp(pkg: string, label: string) {
    setOpenMenuPkg(null);
    mutePackage(pkg);
    setActionMsg(`${label} muted`);
  }

  function handleToggleToasts() {
    const next = { ...appPrefs, desktopNotifications: !appPrefs.desktopNotifications };
    setAppPrefs(next);
    void saveAppPrefs(next).catch(() => {});
  }

  function handleMenuKeyDown(e: React.KeyboardEvent<HTMLDivElement>, pkg: string) {
    const items = Array.from(
      e.currentTarget.querySelectorAll<HTMLButtonElement>("button[role='menuitem']"),
    );
    const idx = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") {
      e.stopPropagation();
      setOpenMenuPkg(null);
      triggerRefs.current[pkg]?.focus();
    } else if (e.key === "ArrowDown" && items.length > 0) {
      e.preventDefault();
      items[(idx + 1) % items.length]?.focus();
    } else if (e.key === "ArrowUp" && items.length > 0) {
      e.preventDefault();
      items[(idx - 1 + items.length) % items.length]?.focus();
    } else if (e.key === "Tab") {
      setOpenMenuPkg(null);
    }
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink-700 bg-ink-900 px-4 py-2">
        <span className="text-xs text-text-muted">
          Read-only — replies need the phone
        </span>
        <div className="flex items-center gap-2">
          {actionMsg && (
            <span className="text-[11px] text-gold-400" role="status">
              {actionMsg}
            </span>
          )}
          <button
            type="button"
            disabled={loading}
            onClick={onRefresh}
            className="btn-tactile inline-flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            Refresh
          </button>
        </div>
      </div>

      {error && (
        <div className="border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-4">
        <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_260px] xl:grid-cols-[minmax(0,1fr)_300px]">
          {/* Left: notification list */}
          <div className="space-y-3">
            {hiddenCount > 0 && (
              <div className="flex items-start gap-2 rounded-lg border border-ink-700 bg-ink-900 px-3 py-2 text-xs text-text">
                <BellOff className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" />
                <span>
                  {hiddenCount} notification{hiddenCount > 1 ? "s" : ""} from muted apps{" "}
                  {hiddenCount > 1 ? "are" : "is"} hidden.
                </span>
              </div>
            )}

            {groups.length === 0 ? (
              <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-ink-700 p-8 text-center">
                <BellOff className="mb-2 h-10 w-10 text-ink-700" />
                <h2 className="text-sm font-semibold text-text-strong">
                  {notifications.length > 0
                    ? "No visible notifications"
                    : "No active notifications"}
                </h2>
                <p className="mt-1 max-w-md text-xs text-text">
                  {notifications.length > 0
                    ? "All apps with active notifications are muted. Unmute an app on the right to see them here."
                    : "Read-only — replies need the phone"}
                </p>
              </div>
            ) : (
              groups.map((group) => {
                const isMenuOpen = openMenuPkg === group.pkg;
                const initial = (group.label[0] || "?").toUpperCase();
                return (
                  <section
                    key={group.pkg}
                    aria-label={group.label}
                    className="relative rounded-xl border border-ink-700 bg-ink-900"
                  >
                    <div className="flex items-center justify-between gap-2 rounded-t-[11px] border-b border-ink-700 bg-ink-800 px-3.5 py-1.5">
                      <span className="flex min-w-0 items-center gap-2 font-semibold text-text-strong">
                        <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-ink-700 bg-ink-900 text-[11px] font-bold text-text-strong">
                          {initial}
                        </span>
                        {group.hasResolvedLabel ? (
                          <span className="truncate text-xs">{group.label}</span>
                        ) : (
                          <>
                            <span className="truncate font-mono text-xs font-medium">
                              {group.pkg}
                            </span>
                            <span className="rounded border border-ink-700 bg-ink-900 px-1.5 py-0.5 font-mono text-[10px] text-text">
                              name unavailable
                            </span>
                          </>
                        )}
                        <span className="rounded border border-ink-700 bg-ink-900 px-1.5 py-0.5 font-mono text-[11px] text-text">
                          {group.items.length}
                        </span>
                      </span>

                      <div className="relative">
                        <button
                          ref={(el) => {
                            triggerRefs.current[group.pkg] = el;
                          }}
                          type="button"
                          aria-haspopup="menu"
                          aria-expanded={isMenuOpen}
                          aria-label={`Mute… ${group.label}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            setOpenMenuPkg(isMenuOpen ? null : group.pkg);
                          }}
                          className="btn-tactile inline-flex items-center gap-1 rounded-md border border-ink-700 bg-ink-900 px-2 py-1 text-[11px] font-medium text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
                        >
                          <BellOff className="h-3 w-3" />
                          Mute…
                          <ChevronDown className="h-3 w-3" />
                        </button>

                        {isMenuOpen && (
                          <div
                            ref={menuRef}
                            role="menu"
                            aria-label="App actions"
                            onKeyDown={(e) => handleMenuKeyDown(e, group.pkg)}
                            className="popover-panel absolute right-0 top-full z-20 mt-1 min-w-[230px] rounded-[10px] border border-ink-700 bg-ink-900 p-1 shadow-elevated"
                            style={{ "--transform-origin": "top right" } as React.CSSProperties}
                          >
                            <button
                              type="button"
                              role="menuitem"
                              onClick={() => void handleOpenApp(group.pkg, group.label)}
                              className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs text-text hover:bg-ink-800 hover:text-text-strong focus-visible:bg-ink-800 focus-visible:text-text-strong cursor-pointer"
                            >
                              <ExternalLink className="h-3.5 w-3.5 shrink-0" />
                              Open {group.label} on PC
                            </button>
                            <hr className="my-1 border-0 border-t border-ink-700" />
                            <button
                              type="button"
                              role="menuitem"
                              onClick={() => handleMuteApp(group.pkg, group.label)}
                              className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs text-text hover:bg-ink-800 hover:text-text-strong focus-visible:bg-ink-800 focus-visible:text-text-strong cursor-pointer"
                            >
                              <BellOff className="h-3.5 w-3.5 shrink-0" />
                              Mute {group.label}
                            </button>
                          </div>
                        )}
                      </div>
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
                              {n.postedUnix > 0 && (
                                <span className="shrink-0 font-mono text-[10px] text-text-muted">
                                  {formatTime(n.postedUnix)}
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
                  </section>
                );
              })
            )}
          </div>

          {/* Right: Muted apps + Desktop toasts preview */}
          <aside className="space-y-4">
            <section
              aria-labelledby="notif-muted-heading"
              className="rounded-xl border border-ink-700 bg-ink-900"
            >
              <div className="flex items-center justify-between gap-2 border-b border-ink-700 px-3.5 py-2.5">
                <h2
                  id="notif-muted-heading"
                  className="text-xs font-semibold text-text-strong"
                >
                  Muted apps
                </h2>
                <span className="rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 font-mono text-[11px] text-text">
                  {mutedPackages.length} muted
                </span>
              </div>
              <div className="p-3.5 text-xs">
                {mutedPackages.length > 0 ? (
                  <>
                    <div className="divide-y divide-ink-700">
                      {mutedPackages.map((pkg) => {
                        const label = labelByPkg.get(pkg) ?? pkg;
                        return (
                          <div
                            key={pkg}
                            className="flex items-center justify-between gap-2 py-1.5 first:pt-0 last:pb-0"
                          >
                            <span className="flex min-w-0 items-center gap-2">
                              <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-ink-700 bg-ink-800 text-[11px] font-bold text-text-strong">
                                {(label[0] || "?").toUpperCase()}
                              </span>
                              <span className="truncate font-medium text-text-strong">
                                {label}
                              </span>
                            </span>
                            <button
                              type="button"
                              onClick={() => {
                                unmutePackage(pkg);
                                setActionMsg(`${label} unmuted`);
                              }}
                              className="btn-tactile shrink-0 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
                            >
                              Unmute
                            </button>
                          </div>
                        );
                      })}
                    </div>
                    <p className="mt-2.5 text-[11px] text-text-muted">
                      Hidden from this list and from desktop toasts.
                    </p>
                  </>
                ) : (
                  <p className="text-text-muted">
                    No muted apps. Use <em>Mute…</em> on any app to hide it here and from desktop toasts.
                  </p>
                )}
              </div>
            </section>

            <section
              aria-labelledby="notif-toasts-heading"
              className="rounded-xl border border-ink-700 bg-ink-900"
            >
              <div className="flex items-center justify-between gap-2 border-b border-ink-700 px-3.5 py-2.5">
                <h2
                  id="notif-toasts-heading"
                  className="text-xs font-semibold text-text-strong"
                >
                  Desktop toasts
                </h2>
                <span className="flex items-center gap-2">
                  <span className="text-[11px] text-text-muted">Show</span>
                  <button
                    type="button"
                    role="switch"
                    aria-label="Show desktop toasts"
                    aria-checked={appPrefs.desktopNotifications}
                    onClick={handleToggleToasts}
                    className={`btn-tactile relative inline-block h-[18px] w-8 shrink-0 rounded-full border-2 border-transparent transition-colors duration-150 ease-out cursor-pointer ${
                      appPrefs.desktopNotifications
                        ? "bg-gold-400"
                        : "bg-ink-800 ring-1 ring-inset ring-ink-700"
                    }`}
                  >
                    <span
                      className={`block h-3.5 w-3.5 rounded-full transition-transform duration-150 ease-out ${
                        appPrefs.desktopNotifications
                          ? "translate-x-3.5 bg-ink-950"
                          : "translate-x-0 bg-text-muted"
                      }`}
                    />
                  </button>
                </span>
              </div>
              <div className="p-3.5 text-xs">
                <p className="mb-2.5 text-text-muted">
                  Preview of the next toast. Muted apps never produce one.
                </p>
                {!appPrefs.desktopNotifications ? (
                  <div className="flex items-start gap-2 rounded-lg border border-ink-700 bg-ink-950 px-3 py-2 text-text">
                    <Info className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" />
                    <span>Desktop toasts are off.</span>
                  </div>
                ) : previewNotification ? (
                  <div
                    role="img"
                    aria-label="Toast preview"
                    className="rounded-[10px] border border-ink-700 bg-ink-950 p-3 shadow-elevated"
                  >
                    <div className="mb-1.5 flex items-center gap-1.5 text-[11px] text-text">
                      <span className="flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded border border-ink-700 bg-ink-800 text-[10px] font-bold text-text-strong">
                        {((previewNotification.appLabel || previewNotification.package)[0] || "?").toUpperCase()}
                      </span>
                      <span className="truncate">
                        {previewNotification.appLabel || previewNotification.package}
                      </span>
                      <span className="ml-auto text-text-muted">now</span>
                    </div>
                    <div className="font-semibold text-text-strong">
                      {previewNotification.title || "(No title)"}
                    </div>
                    {previewNotification.text && (
                      <div className="mt-0.5 text-text">{previewNotification.text}</div>
                    )}
                  </div>
                ) : (
                  <div className="flex items-start gap-2 rounded-lg border border-ink-700 bg-ink-950 px-3 py-2 text-text">
                    <Info className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" />
                    <span>Nothing to preview — every app is muted or no notifications exist.</span>
                  </div>
                )}
              </div>
            </section>
          </aside>
        </div>
      </div>
    </div>
  );
}
