import { useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  BellOff,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  ExternalLink,
  Eye,
  EyeOff,
  Info,
  Lock,
  RefreshCw,
  Search,
  Shield,
  ShieldOff,
  WifiOff,
  X,
} from "lucide-react";
import {
  phoneDeleteAllNotificationData,
  phoneGetNotificationHistory,
  phoneOpenApp,
  phoneSetNotificationHidden,
} from "@/services/phoneHubService";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import { useAppStore } from "@/store/useAppStore";
import { saveAppPrefs, saveNotificationStorage } from "@/lib/persist";
import {
  formatMoneySourceLabel,
  formatRetentionBadge,
  withUpdatedAppPolicy,
} from "@/lib/notificationStorage";
import { describeOpenApp } from "@/lib/openApp";
import { logActivity } from "@/stores/activityStore";
import {
  bucketByLocalDay,
  collapseNotificationWhitespace,
  filterNotifications,
  formatFullNotificationTime,
  formatRelativeNotificationTime,
  getNotificationHideId,
  shouldShowNotificationExpand,
  stackSameTitleNotifications,
} from "@/lib/notificationTidy";
import { AppAvatar } from "./AppAvatar";
import type { PhoneHubNotification, StoredNotification } from "@/types/phone";

const VISIBLE_BATCH_SIZE = 50;

interface NotificationsViewProps {
  serial: string;
  hwSerial?: string;
  isOffline?: boolean;
  notifications: PhoneHubNotification[];
  loading: boolean;
  error: string | null;
  onRefresh: () => void;
  onOpenSettings?: () => void;
}

interface NotificationRowProps {
  item: PhoneHubNotification;
  hideId: string;
  nowMs: number;
  isSensitive: boolean;
  isRevealed: boolean;
  isBodyExpanded: boolean;
  isCopied: boolean;
  isNested?: boolean;
  onReveal: (id: string) => void;
  onMask: (id: string) => void;
  onToggleExpand: (id: string) => void;
  onCopy: (item: PhoneHubNotification, hideId: string) => void;
  onHide: (hideId: string) => void;
}

function NotificationRow({
  item,
  hideId,
  nowMs,
  isSensitive,
  isRevealed,
  isBodyExpanded,
  isCopied,
  isNested = false,
  onReveal,
  onMask,
  onToggleExpand,
  onCopy,
  onHide,
}: NotificationRowProps) {
  const relTime = formatRelativeNotificationTime(item.postedUnix, nowMs);
  const fullTime = formatFullNotificationTime(item.postedUnix);
  const collapsedText = collapseNotificationWhitespace(item.text);
  const canExpand = shouldShowNotificationExpand(item.text, collapsedText);

  return (
    <div
      className={`flex items-start justify-between gap-3 ${
        isNested ? "rounded-lg border border-ink-700 bg-ink-950 px-3 py-2" : ""
      }`}
    >
      <div className="min-w-0 flex-1">
        {!isRevealed ? (
          <div className="space-y-1.5">
            <div className="flex flex-wrap items-center gap-2">
              <span className="inline-flex items-center gap-1 rounded border border-gold-400/40 bg-ink-950 px-1.5 py-0.5 font-mono text-[10px] text-gold-400">
                <Lock className="h-2.5 w-2.5" />
                Sensitive
              </span>
              {relTime && (
                <span
                  title={fullTime}
                  aria-label={fullTime}
                  className="shrink-0 font-mono text-[10px] text-text-muted"
                >
                  {relTime}
                </span>
              )}
            </div>
            <button
              type="button"
              aria-expanded={false}
              onClick={() => onReveal(hideId)}
              className="btn-tactile inline-flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs text-text hover:border-gold-400 hover:text-text-strong cursor-pointer"
            >
              <Eye className="h-3.5 w-3.5 text-gold-400" />
              Sensitive content hidden — click to reveal
            </button>
          </div>
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <span className="truncate text-xs font-medium text-text-strong">
                {item.title || "(No title)"}
              </span>
              {relTime && (
                <span
                  title={fullTime}
                  aria-label={fullTime}
                  className="shrink-0 font-mono text-[10px] text-text-muted"
                >
                  {relTime}
                </span>
              )}
              {isSensitive && (
                <button
                  type="button"
                  aria-expanded={true}
                  onClick={() => onMask(hideId)}
                  className="btn-tactile inline-flex items-center gap-1 rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
                >
                  <EyeOff className="h-2.5 w-2.5" />
                  Mask
                </button>
              )}
            </div>

            {collapsedText && (
              <>
                <p
                  className={
                    isBodyExpanded
                      ? "notif-expand-panel mt-0.5 whitespace-pre-wrap break-words text-xs text-text"
                      : "mt-0.5 line-clamp-2 whitespace-pre-line break-words text-xs text-text-muted"
                  }
                >
                  {isBodyExpanded ? item.text : collapsedText}
                </p>
                {canExpand && (
                  <button
                    type="button"
                    aria-expanded={isBodyExpanded}
                    onClick={() => onToggleExpand(hideId)}
                    className="mt-1 inline-flex items-center gap-1 rounded text-[11px] font-medium text-gold-400 hover:text-text-strong cursor-pointer"
                  >
                    {isBodyExpanded ? "Show less" : "Show more"}
                  </button>
                )}
              </>
            )}
          </>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-1.5">
        {isRevealed && (
          <button
            type="button"
            onClick={() => onCopy(item, hideId)}
            className="btn-tactile flex shrink-0 items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
          >
            {isCopied ? (
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
        )}
        <button
          type="button"
          aria-label="Hide in AnyLeap"
          title="Hide in AnyLeap"
          onClick={() => onHide(hideId)}
          className="btn-tactile flex shrink-0 items-center justify-center rounded-md border border-ink-700 bg-ink-800 p-1 text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
        >
          <EyeOff className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}

export function NotificationsView({
  serial,
  hwSerial,
  isOffline = false,
  notifications,
  loading,
  error,
  onRefresh,
  onOpenSettings,
}: NotificationsViewProps) {
  const mutedPackages = usePhoneHubStore((s) => s.mutedPackages);
  const mutePackage = usePhoneHubStore((s) => s.mutePackage);
  const unmutePackage = usePhoneHubStore((s) => s.unmutePackage);
  const sensitivePackages = usePhoneHubStore((s) => s.sensitivePackages);
  const togglePackageSensitive = usePhoneHubStore((s) => s.togglePackageSensitive);
  const hiddenNotificationIds = usePhoneHubStore((s) => s.hiddenNotificationIds);
  const hideNotificationLocal = usePhoneHubStore((s) => s.hideNotificationLocal);
  const unhideAllLocal = usePhoneHubStore((s) => s.unhideAllLocal);

  const appPrefs = useAppStore((s) => s.appPrefs);
  const setAppPrefs = useAppStore((s) => s.setAppPrefs);
  const notificationStorage = useAppStore((s) => s.notificationStorage);
  const setNotificationStorage = useAppStore((s) => s.setNotificationStorage);
  const vaultError = useAppStore((s) => s.vaultError);
  const setVaultError = useAppStore((s) => s.setVaultError);

  const [savedHistory, setSavedHistory] = useState<StoredNotification[]>([]);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [openMenuPkg, setOpenMenuPkg] = useState<string | null>(null);
  const [actionMsg, setActionMsg] = useState<string | null>(null);
  const [selectedPackage, setSelectedPackage] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(() => new Set());
  const [expandedBodies, setExpandedBodies] = useState<Set<string>>(() => new Set());
  const [expandedStacks, setExpandedStacks] = useState<Set<string>>(() => new Set());
  const [revealedIds, setRevealedIds] = useState<Set<string>>(() => new Set());
  const [visibleLimit, setVisibleLimit] = useState(VISIBLE_BATCH_SIZE);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const triggerRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const menuRef = useRef<HTMLDivElement | null>(null);

  const effectiveHwSerial = hwSerial || serial;

  // Hydrate DPAPI-encrypted saved history when master opt-in is enabled.
  useEffect(() => {
    if (!notificationStorage.historyEnabled || !effectiveHwSerial) {
      setSavedHistory([]);
      return;
    }
    let cancelled = false;
    void phoneGetNotificationHistory(effectiveHwSerial, 200)
      .then((items) => {
        if (!cancelled && Array.isArray(items)) {
          setSavedHistory(items);
        }
      })
      .catch((err) => {
        const msg = String(err);
        if (!cancelled && msg.includes("VAULT_DECRYPT_FAILED")) {
          setVaultError(msg);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [
    notificationStorage.historyEnabled,
    effectiveHwSerial,
    notifications.length,
    setVaultError,
  ]);

  const { mergedNotifications, dedupeIdByHideId, persistedHiddenIds } = useMemo(() => {
    const byHideId = new Map<string, PhoneHubNotification>();
    const dedupeMap = new Map<string, string>();
    const vaultHidden = new Set<string>();

    for (const saved of savedHistory) {
      const item: PhoneHubNotification = {
        key: saved.key,
        package: saved.package,
        appLabel: saved.appLabel,
        title: saved.title,
        text: saved.text,
        postedUnix: saved.postedUnix,
      };
      const hideId = getNotificationHideId(item);
      byHideId.set(hideId, item);
      dedupeMap.set(hideId, saved.dedupeId);
      if (saved.hiddenLocal) {
        vaultHidden.add(hideId);
      }
    }

    for (const live of notifications) {
      const hideId = getNotificationHideId(live);
      if (!byHideId.has(hideId)) {
        byHideId.set(hideId, live);
      } else {
        byHideId.set(hideId, live);
      }
    }

    return {
      mergedNotifications: Array.from(byHideId.values()),
      dedupeIdByHideId: dedupeMap,
      persistedHiddenIds: vaultHidden,
    };
  }, [notifications, savedHistory]);

  const mutedSet = useMemo(() => new Set(mutedPackages), [mutedPackages]);
  const sensitiveSet = useMemo(() => {
    const next = new Set(sensitivePackages);
    for (const [pkg, pol] of Object.entries(notificationStorage.perApp)) {
      if (pol.sensitive) next.add(pkg);
    }
    return next;
  }, [sensitivePackages, notificationStorage.perApp]);
  const hiddenSet = useMemo(() => {
    const next = new Set(hiddenNotificationIds);
    for (const id of persistedHiddenIds) {
      next.add(id);
    }
    return next;
  }, [hiddenNotificationIds, persistedHiddenIds]);

  // Re-mask ephemeral reveal state and reset per-device transient UI state on device switch.
  useEffect(() => {
    setRevealedIds(new Set());
    setSelectedPackage(null);
    setVisibleLimit(VISIBLE_BATCH_SIZE);
  }, [serial]);

  // Keep relative timestamps fresh every 30s while viewing Notifications.
  useEffect(() => {
    setNowMs(Date.now());
    const id = setInterval(() => setNowMs(Date.now()), 30_000);
    return () => clearInterval(id);
  }, []);

  // Remember app labels seen in notifications for muted summary and filter chips.
  const labelByPkg = useMemo(() => {
    const map = new Map<string, string>();
    for (const n of mergedNotifications) {
      if (n.appLabel && n.appLabel !== n.package) {
        map.set(n.package, n.appLabel);
      }
    }
    return map;
  }, [mergedNotifications]);

  const mutedHiddenCount = useMemo(
    () => mergedNotifications.filter((n) => mutedSet.has(n.package)).length,
    [mergedNotifications, mutedSet],
  );

  const visibleNotifications = useMemo(
    () =>
      mergedNotifications
        .filter((n) => !mutedSet.has(n.package))
        .filter((n) => !hiddenSet.has(getNotificationHideId(n)))
        .sort((a, b) => b.postedUnix - a.postedUnix),
    [mergedNotifications, mutedSet, hiddenSet],
  );

  const locallyHiddenCount = useMemo(
    () =>
      mergedNotifications.filter(
        (n) => !mutedSet.has(n.package) && hiddenSet.has(getNotificationHideId(n)),
      ).length,
    [mergedNotifications, mutedSet, hiddenSet],
  );

  // Per-app chips computed from non-muted, non-locally-hidden notifications.
  const appChips = useMemo(() => {
    const map = new Map<
      string,
      { pkg: string; label: string; count: number; latestUnix: number }
    >();
    for (const n of visibleNotifications) {
      const existing = map.get(n.package);
      const label = labelByPkg.get(n.package) ?? (n.appLabel || n.package);
      if (existing) {
        existing.count += 1;
        if (n.postedUnix > existing.latestUnix) existing.latestUnix = n.postedUnix;
      } else {
        map.set(n.package, {
          pkg: n.package,
          label,
          count: 1,
          latestUnix: n.postedUnix,
        });
      }
    }
    return Array.from(map.values()).sort((a, b) => b.latestUnix - a.latestUnix);
  }, [visibleNotifications, labelByPkg]);

  // Clear selected chip if that app no longer has visible notifications.
  useEffect(() => {
    if (selectedPackage && !appChips.some((c) => c.pkg === selectedPackage)) {
      setSelectedPackage(null);
    }
  }, [selectedPackage, appChips]);

  // Build masked package set (apps marked sensitive where not all items are revealed)
  // so search does not leak masked body/title content.
  const searchMaskedPackages = useMemo(() => {
    const masked = new Set<string>();
    for (const pkg of sensitiveSet) {
      const hasUnrevealed = visibleNotifications.some(
        (n) => n.package === pkg && !revealedIds.has(getNotificationHideId(n)),
      );
      if (hasUnrevealed) masked.add(pkg);
    }
    return masked;
  }, [sensitiveSet, visibleNotifications, revealedIds]);

  const filteredNotifications = useMemo(
    () =>
      filterNotifications(visibleNotifications, {
        selectedPackage,
        query: searchQuery,
        maskedPackages: searchMaskedPackages,
      }),
    [visibleNotifications, selectedPackage, searchQuery, searchMaskedPackages],
  );

  const windowedNotifications = useMemo(
    () => filteredNotifications.slice(0, visibleLimit),
    [filteredNotifications, visibleLimit],
  );

  const daySections = useMemo(() => {
    const buckets = bucketByLocalDay(windowedNotifications, nowMs);
    return buckets.map((bucket) => {
      const groupMap = new Map<
        string,
        {
          pkg: string;
          label: string;
          hasResolvedLabel: boolean;
          items: PhoneHubNotification[];
        }
      >();
      for (const n of bucket.items) {
        const existing = groupMap.get(n.package);
        if (existing) {
          existing.items.push(n);
        } else {
          const resolvedLabel = labelByPkg.get(n.package) ?? n.appLabel;
          const hasResolvedLabel = Boolean(resolvedLabel && resolvedLabel !== n.package);
          groupMap.set(n.package, {
            pkg: n.package,
            label: resolvedLabel || n.package,
            hasResolvedLabel,
            items: [n],
          });
        }
      }
      return {
        id: bucket.id,
        label: bucket.label,
        totalCount: bucket.items.length,
        groups: Array.from(groupMap.values()),
      };
    });
  }, [windowedNotifications, nowMs, labelByPkg]);

  const previewNotification = visibleNotifications[0] ?? null;
  const previewIsSensitive = previewNotification
    ? sensitiveSet.has(previewNotification.package)
    : false;

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

  async function handleCopy(n: PhoneHubNotification, hideId: string) {
    const content = [n.title, n.text].filter(Boolean).join("\n");
    try {
      await navigator.clipboard.writeText(content);
      setCopiedKey(hideId);
      setTimeout(() => setCopiedKey((prev) => (prev === hideId ? null : prev)), 1500);
    } catch {
      // Clipboard write failed
    }
  }

  async function handleOpenApp(pkg: string, label: string) {
    setOpenMenuPkg(null);
    try {
      const res = await phoneOpenApp(serial, pkg);
      const msg = describeOpenApp(label, res?.outcome);
      setActionMsg(msg);
      logActivity({ kind: "app", title: msg, detail: pkg });
    } catch (e) {
      setActionMsg(String(e));
    }
  }

  function handleMuteApp(pkg: string, label: string) {
    setOpenMenuPkg(null);
    mutePackage(pkg);
    setActionMsg(`${label} muted`);
  }

  function handleToggleSensitive(pkg: string, label: string) {
    const wasSensitive = sensitiveSet.has(pkg);
    setOpenMenuPkg(null);
    togglePackageSensitive(pkg);
    if (notificationStorage.perApp[pkg]) {
      const next = withUpdatedAppPolicy(notificationStorage, pkg, (p) => ({
        ...p,
        sensitive: !wasSensitive,
      }));
      setNotificationStorage(next);
      void saveNotificationStorage(next).catch(() => {});
    }
    setRevealedIds((prev) => {
      const next = new Set(prev);
      for (const id of prev) {
        if (id.startsWith(`${pkg}::`)) next.delete(id);
      }
      return next;
    });
    setActionMsg(
      wasSensitive
        ? `${label} unmarked as sensitive`
        : `${label} marked as sensitive`,
    );
  }

  function handleHideItem(hideId: string) {
    hideNotificationLocal(hideId);
    const dedupeId = dedupeIdByHideId.get(hideId);
    if (notificationStorage.historyEnabled && dedupeId) {
      setSavedHistory((prev) =>
        prev.map((s) => (s.dedupeId === dedupeId ? { ...s, hiddenLocal: true } : s)),
      );
      void phoneSetNotificationHidden(dedupeId, true).catch(() => {});
    }
  }

  function handleUnhideAll() {
    unhideAllLocal();
    if (notificationStorage.historyEnabled) {
      const hiddenSaved = savedHistory.filter((s) => s.hiddenLocal);
      if (hiddenSaved.length > 0) {
        setSavedHistory((prev) => prev.map((s) => ({ ...s, hiddenLocal: false })));
        for (const s of hiddenSaved) {
          void phoneSetNotificationHidden(s.dedupeId, false).catch(() => {});
        }
      }
    }
  }

  async function handleResetCorruptVault() {
    try {
      await phoneDeleteAllNotificationData();
      setVaultError(null);
      setSavedHistory([]);
      setActionMsg("Local vault reinitialized.");
    } catch (e) {
      setActionMsg(String(e));
    }
  }

  function handleToggleToasts() {
    const next = { ...appPrefs, desktopNotifications: !appPrefs.desktopNotifications };
    setAppPrefs(next);
    void saveAppPrefs(next).catch(() => {});
  }

  function handleMenuKeyDown(e: React.KeyboardEvent<HTMLDivElement>, menuKey: string) {
    const items = Array.from(
      e.currentTarget.querySelectorAll<HTMLButtonElement>("button[role='menuitem']"),
    );
    const idx = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") {
      e.stopPropagation();
      setOpenMenuPkg(null);
      triggerRefs.current[menuKey]?.focus();
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

  function toggleGroupCollapsed(groupKey: string) {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(groupKey)) next.delete(groupKey);
      else next.add(groupKey);
      return next;
    });
  }

  function toggleBodyExpand(hideId: string) {
    setExpandedBodies((prev) => {
      const next = new Set(prev);
      if (next.has(hideId)) next.delete(hideId);
      else next.add(hideId);
      return next;
    });
  }

  function toggleStackExpand(stackId: string) {
    setExpandedStacks((prev) => {
      const next = new Set(prev);
      if (next.has(stackId)) next.delete(stackId);
      else next.add(stackId);
      return next;
    });
  }

  function revealItem(hideId: string) {
    setRevealedIds((prev) => new Set(prev).add(hideId));
  }

  function maskItem(hideId: string) {
    setRevealedIds((prev) => {
      const next = new Set(prev);
      next.delete(hideId);
      return next;
    });
  }

  const remainingWindowCount = filteredNotifications.length - windowedNotifications.length;
  const hasFiltersActive = Boolean(selectedPackage || searchQuery.trim());

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink-700 bg-ink-900 px-4 py-2">
        <span
          title="Read-only — replies and dismisses stay on the phone"
          className="text-xs text-text-muted"
        >
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
            disabled={loading || isOffline}
            title={isOffline ? "Connect phone to refresh live notifications" : undefined}
            onClick={onRefresh}
            className="btn-tactile inline-flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
          >
            <RefreshCw
              className={`h-3.5 w-3.5 ${loading ? "animate-spin motion-reduce:animate-none" : ""}`}
            />
            Refresh
          </button>
        </div>
      </div>

      {isOffline && (
        <div
          role="status"
          className="flex items-center gap-2 border-b border-ink-700 bg-ink-900 px-4 py-2 text-xs text-mist-300"
        >
          <WifiOff className="h-3.5 w-3.5 shrink-0" />
          <span>Offline — showing saved history</span>
        </div>
      )}

      {vaultError && (
        <div
          role="alert"
          className="flex flex-wrap items-center justify-between gap-2 border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger"
        >
          <div className="flex items-center gap-2">
            <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
            <span>
              Unable to decrypt local vault for this Windows user. Delete &amp; reinitialize to
              start fresh.
            </span>
          </div>
          <button
            type="button"
            onClick={() => void handleResetCorruptVault()}
            className="btn-tactile rounded border border-status-danger/50 bg-ink-950 px-2 py-0.5 text-[11px] font-semibold text-status-danger hover:bg-ink-900 cursor-pointer"
          >
            Delete &amp; reinitialize
          </button>
        </div>
      )}

      {error && (
        <div className="border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-4">
        <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_260px] xl:grid-cols-[minmax(0,1fr)_300px]">
          {/* Left: search, filter chips, and tidy notification sections */}
          <div className="space-y-3">
            {/* Search & local-hidden bar */}
            <div className="flex flex-wrap items-center gap-2">
              <div
                role="search"
                className="relative flex min-w-[200px] flex-1 items-center"
              >
                <Search
                  aria-hidden="true"
                  className="pointer-events-none absolute left-2.5 h-3.5 w-3.5 text-text-muted"
                />
                <input
                  type="search"
                  aria-label="Search notifications"
                  placeholder="Search notifications, apps, text…"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape" && searchQuery) {
                      e.preventDefault();
                      e.stopPropagation();
                      setSearchQuery("");
                    }
                  }}
                  className="w-full rounded-lg border border-ink-700 bg-ink-900 py-1.5 pr-8 pl-8 text-xs text-text-strong placeholder:text-text-muted focus:border-gold-400 focus:outline-none"
                />
                {searchQuery && (
                  <button
                    type="button"
                    aria-label="Clear search"
                    onClick={() => setSearchQuery("")}
                    className="absolute right-2 rounded p-0.5 text-text-muted hover:text-text-strong cursor-pointer"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                )}
              </div>

              {locallyHiddenCount > 0 && (
                <button
                  type="button"
                  onClick={handleUnhideAll}
                  className="btn-tactile inline-flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-900 px-2.5 py-1.5 text-xs text-gold-400 hover:border-gold-400 cursor-pointer"
                >
                  <Eye className="h-3.5 w-3.5" />
                  {locallyHiddenCount} hidden · Show
                </button>
              )}
            </div>

            {/* App filter chips */}
            {appChips.length > 0 && (
              <div
                role="group"
                aria-label="Filter notifications by app"
                className="flex flex-wrap items-center gap-1.5"
              >
                <button
                  type="button"
                  aria-pressed={selectedPackage === null}
                  onClick={() => setSelectedPackage(null)}
                  className={`btn-tactile inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium cursor-pointer ${
                    selectedPackage === null
                      ? "border-gold-400 bg-ink-800 text-gold-400"
                      : "border-ink-700 bg-ink-900 text-text hover:border-text-muted hover:text-text-strong"
                  }`}
                >
                  All
                  <span className="font-mono text-[10px]">{visibleNotifications.length}</span>
                </button>
                {appChips.map((chip) => {
                  const isActive = selectedPackage === chip.pkg;
                  return (
                    <button
                      key={chip.pkg}
                      type="button"
                      aria-pressed={isActive}
                      onClick={() => setSelectedPackage(isActive ? null : chip.pkg)}
                      className={`btn-tactile inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium cursor-pointer ${
                        isActive
                          ? "border-gold-400 bg-ink-800 text-gold-400"
                          : "border-ink-700 bg-ink-900 text-text hover:border-text-muted hover:text-text-strong"
                      }`}
                    >
                      <AppAvatar
                        label={chip.label}
                        packageName={chip.pkg}
                        sizeClass="h-4 w-4 text-[9px]"
                      />
                      <span className="max-w-[140px] truncate">{chip.label}</span>
                      <span className="font-mono text-[10px]">{chip.count}</span>
                    </button>
                  );
                })}
              </div>
            )}

            {mutedHiddenCount > 0 && (
              <div className="flex items-start gap-2 rounded-lg border border-ink-700 bg-ink-900 px-3 py-2 text-xs text-text">
                <BellOff className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" />
                <span>
                  {mutedHiddenCount} notification{mutedHiddenCount > 1 ? "s" : ""} from muted apps{" "}
                  {mutedHiddenCount > 1 ? "are" : "is"} hidden.
                </span>
              </div>
            )}

            {daySections.length === 0 ? (
              <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-ink-700 p-8 text-center">
                <BellOff className="mb-2 h-10 w-10 text-ink-700" />
                <h2 className="text-sm font-semibold text-text-strong">
                  {hasFiltersActive
                    ? "No matching notifications"
                    : mergedNotifications.length > 0
                      ? "No visible notifications"
                      : "No active notifications"}
                </h2>
                <p className="mt-1 max-w-md text-xs text-text">
                  {hasFiltersActive
                    ? "Try clearing the search box or choosing All apps."
                    : mutedHiddenCount > 0 && locallyHiddenCount === 0
                      ? "All apps with active notifications are muted. Unmute an app on the right to see them here."
                      : locallyHiddenCount > 0
                        ? "All visible notifications are hidden locally in AnyLeap."
                        : "Read-only — replies and dismisses stay on the phone"}
                </p>
                {hasFiltersActive && (
                  <button
                    type="button"
                    onClick={() => {
                      setSelectedPackage(null);
                      setSearchQuery("");
                    }}
                    className="btn-tactile mt-3 rounded-md border border-ink-700 bg-ink-800 px-3 py-1 text-xs text-text hover:border-gold-400 hover:text-text-strong cursor-pointer"
                  >
                    Clear filters
                  </button>
                )}
              </div>
            ) : (
              <div className="space-y-4">
                {daySections.map((section) => (
                  <section
                    key={section.id}
                    aria-labelledby={`notif-day-${section.id}`}
                    className="space-y-2.5"
                  >
                    <div className="flex items-center justify-between px-0.5">
                      <h2
                        id={`notif-day-${section.id}`}
                        className="font-mono text-[11px] font-bold uppercase tracking-wider text-text-muted"
                      >
                        {section.label}
                      </h2>
                      <span className="font-mono text-[11px] text-text-muted">
                        {section.totalCount}
                      </span>
                    </div>

                    <div className="space-y-2.5">
                      {section.groups.map((group) => {
                        const groupKey = `${section.id}::${group.pkg}`;
                        const isCollapsed = collapsedGroups.has(groupKey);
                        const isMenuOpen = openMenuPkg === groupKey;
                        const isSensitive = sensitiveSet.has(group.pkg);
                        const stacks = stackSameTitleNotifications(group.items);

                        return (
                          <section
                            key={groupKey}
                            aria-label={group.label}
                            className="relative rounded-xl border border-ink-700 bg-ink-900"
                          >
                            <div
                              className={`flex flex-wrap items-center justify-between gap-2 bg-ink-800 px-3.5 py-1.5 ${
                                isCollapsed
                                  ? "rounded-[11px]"
                                  : "rounded-t-[11px] border-b border-ink-700"
                              }`}
                            >
                              <button
                                type="button"
                                aria-expanded={!isCollapsed}
                                aria-label={`${isCollapsed ? "Expand" : "Collapse"} ${group.label} notifications`}
                                onClick={() => toggleGroupCollapsed(groupKey)}
                                className="flex min-w-0 flex-1 items-center gap-2 text-left font-semibold text-text-strong cursor-pointer"
                              >
                                {isCollapsed ? (
                                  <ChevronRight className="h-3.5 w-3.5 shrink-0 text-text" />
                                ) : (
                                  <ChevronDown className="h-3.5 w-3.5 shrink-0 text-text" />
                                )}
                                <AppAvatar
                                  label={group.label}
                                  packageName={group.pkg}
                                />
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
                              </button>

                              <div className="flex shrink-0 items-center gap-1.5">
                                {isSensitive && (
                                  <span className="inline-flex items-center gap-1 rounded border border-gold-400/40 bg-ink-900 px-1.5 py-0.5 font-mono text-[10px] text-gold-400">
                                    <Lock className="h-2.5 w-2.5" />
                                    Sensitive
                                  </span>
                                )}
                                {(notificationStorage.perApp[group.pkg]?.moneySources ?? [])
                                  .filter((s) => s.enabled)
                                  .map((src) => (
                                    <span
                                      key={src.id}
                                      className="rounded border border-gold-400/40 bg-ink-900 px-1.5 py-0.5 font-mono text-[10px] text-gold-400"
                                    >
                                      {formatMoneySourceLabel(src)}
                                    </span>
                                  ))}
                                {onOpenSettings ? (
                                  <button
                                    type="button"
                                    onClick={onOpenSettings}
                                    className="rounded border border-ink-700 bg-ink-900 px-1.5 py-0.5 font-mono text-[10px] text-text hover:border-gold-400 hover:text-gold-400 cursor-pointer"
                                  >
                                    {formatRetentionBadge(notificationStorage.perApp[group.pkg])} ·
                                    Settings
                                  </button>
                                ) : (
                                  <span className="rounded border border-ink-700 bg-ink-900 px-1.5 py-0.5 font-mono text-[10px] text-text">
                                    {formatRetentionBadge(notificationStorage.perApp[group.pkg])} ·
                                    Settings
                                  </span>
                                )}

                                <div className="relative">
                                  <button
                                    ref={(el) => {
                                      triggerRefs.current[groupKey] = el;
                                    }}
                                    type="button"
                                    aria-haspopup="menu"
                                    aria-expanded={isMenuOpen}
                                    aria-label={`Mute… ${group.label}`}
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      setOpenMenuPkg(isMenuOpen ? null : groupKey);
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
                                      onKeyDown={(e) => handleMenuKeyDown(e, groupKey)}
                                      className="popover-panel absolute right-0 top-full z-20 mt-1 min-w-[230px] rounded-[10px] border border-ink-700 bg-ink-900 p-1 shadow-elevated"
                                      style={
                                        { "--transform-origin": "top right" } as React.CSSProperties
                                      }
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
                                      <button
                                        type="button"
                                        role="menuitem"
                                        onClick={() =>
                                          handleToggleSensitive(group.pkg, group.label)
                                        }
                                        className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-xs text-text hover:bg-ink-800 hover:text-text-strong focus-visible:bg-ink-800 focus-visible:text-text-strong cursor-pointer"
                                      >
                                        {isSensitive ? (
                                          <>
                                            <ShieldOff className="h-3.5 w-3.5 shrink-0" />
                                            Unmark {group.label} as sensitive
                                          </>
                                        ) : (
                                          <>
                                            <Shield className="h-3.5 w-3.5 shrink-0" />
                                            Mark {group.label} as sensitive
                                          </>
                                        )}
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
                            </div>

                            {!isCollapsed && (
                              <div className="notif-expand-panel divide-y divide-ink-700">
                                {stacks.map((stack) => {
                                  const latestHideId = getNotificationHideId(stack.latest);
                                  const latestRevealed =
                                    !isSensitive || revealedIds.has(latestHideId);
                                  const isStackExpanded = expandedStacks.has(stack.id);

                                  return (
                                    <div key={stack.id} className="px-3.5 py-2.5">
                                      <NotificationRow
                                        item={stack.latest}
                                        hideId={latestHideId}
                                        nowMs={nowMs}
                                        isSensitive={isSensitive}
                                        isRevealed={latestRevealed}
                                        isBodyExpanded={expandedBodies.has(latestHideId)}
                                        isCopied={copiedKey === latestHideId}
                                        onReveal={revealItem}
                                        onMask={maskItem}
                                        onToggleExpand={toggleBodyExpand}
                                        onCopy={(item, id) => void handleCopy(item, id)}
                                        onHide={handleHideItem}
                                      />

                                      {stack.olderItems.length > 0 && (
                                        <div className="mt-2">
                                          <button
                                            type="button"
                                            aria-expanded={isStackExpanded}
                                            onClick={() => toggleStackExpand(stack.id)}
                                            className="btn-tactile inline-flex items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-0.5 font-mono text-[11px] text-text hover:border-gold-400 hover:text-gold-400 cursor-pointer"
                                          >
                                            {isStackExpanded
                                              ? `Show fewer (${stack.olderItems.length})`
                                              : `+${stack.olderItems.length} more`}
                                          </button>

                                          {isStackExpanded && (
                                            <div className="notif-expand-panel mt-2 space-y-2 border-l-2 border-ink-700 pl-3">
                                              {stack.olderItems.map((older) => {
                                                const olderHideId = getNotificationHideId(older);
                                                const olderRevealed =
                                                  !isSensitive || revealedIds.has(olderHideId);
                                                return (
                                                  <NotificationRow
                                                    key={olderHideId}
                                                    item={older}
                                                    hideId={olderHideId}
                                                    nowMs={nowMs}
                                                    isSensitive={isSensitive}
                                                    isRevealed={olderRevealed}
                                                    isBodyExpanded={expandedBodies.has(olderHideId)}
                                                    isCopied={copiedKey === olderHideId}
                                                    isNested
                                                    onReveal={revealItem}
                                                    onMask={maskItem}
                                                    onToggleExpand={toggleBodyExpand}
                                                    onCopy={(item, id) => void handleCopy(item, id)}
                                                    onHide={handleHideItem}
                                                  />
                                                );
                                              })}
                                            </div>
                                          )}
                                        </div>
                                      )}
                                    </div>
                                  );
                                })}
                              </div>
                            )}
                          </section>
                        );
                      })}
                    </div>
                  </section>
                ))}

                {remainingWindowCount > 0 && (
                  <div className="flex justify-center pt-1">
                    <button
                      type="button"
                      onClick={() => setVisibleLimit((prev) => prev + VISIBLE_BATCH_SIZE)}
                      className="btn-tactile rounded-md border border-ink-700 bg-ink-900 px-3 py-1.5 text-xs text-text hover:border-gold-400 hover:text-text-strong cursor-pointer"
                    >
                      Show older ({remainingWindowCount} remaining)
                    </button>
                  </div>
                )}
              </div>
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
                              <AppAvatar
                                label={label}
                                packageName={pkg}
                              />
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
                      <AppAvatar
                        label={previewNotification.appLabel || previewNotification.package}
                        packageName={previewNotification.package}
                        sizeClass="h-[18px] w-[18px] text-[10px]"
                      />
                      <span className="truncate">
                        {previewNotification.appLabel || previewNotification.package}
                      </span>
                      <span className="ml-auto text-text-muted">now</span>
                    </div>
                    {previewIsSensitive ? (
                      <>
                        <div className="font-semibold text-gold-400">
                          Sensitive content hidden
                        </div>
                        <div className="mt-0.5 text-text">
                          New notification from{" "}
                          {previewNotification.appLabel || previewNotification.package}
                        </div>
                      </>
                    ) : (
                      <>
                        <div className="font-semibold text-text-strong">
                          {previewNotification.title || "(No title)"}
                        </div>
                        {previewNotification.text && (
                          <div className="mt-0.5 text-text">
                            {collapseNotificationWhitespace(previewNotification.text)}
                          </div>
                        )}
                      </>
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
