import { useEffect, useId, useMemo, useState } from "react";
import {
  AlertTriangle,
  ChevronDown,
  Plus,
  RefreshCw,
  Search,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import { saveAppPrefs, saveNotificationStorage, type AppPrefs } from "@/lib/persist";
import {
  clampTransferWindowMinutes,
  createMoneySourceId,
  formatRuleTemplateLabel,
  getAppPolicy,
  isPolicyDefaultEmpty,
  isValidAndroidPackage,
  migrateInvalidPerAppEntries,
  MONEY_SOURCE_TYPE_LABELS,
  RULE_TEMPLATES_BY_TYPE,
  withUpdatedAppPolicy,
  type AppNotificationPolicy,
  type MoneySourceType,
  type NotificationStorageSettings,
  type RetentionDays,
  type RetentionPolicy,
  type TransactionRetention,
} from "@/lib/notificationStorage";
import {
  phoneDeleteAllNotificationData,
  phoneDeleteAppNotificationData,
  phoneGetSavedNotificationPackages,
  phoneHubSetSettings,
  phoneListApps,
  phoneSetNotificationStorageConfig,
} from "@/services/phoneHubService";
import { selectClass, optionClass } from "@/lib/styles";
import { entry } from "./registry";
import { Panel } from "./Panel";
import { SegmentedControl } from "./SegmentedControl";
import { SwitchRow } from "./SwitchRow";
import type { ShowFn } from "./SettingsShell";

const POLL_INTERVAL_SEGMENTS = [
  { value: 5, label: "5 s" },
  { value: 10, label: "10 s" },
  { value: 30, label: "30 s" },
];

const TX_RETENTION_SEGMENTS: Array<{ value: TransactionRetention; label: string }> = [
  { value: "1y", label: "1 year" },
  { value: "2y", label: "2 years" },
  { value: "forever", label: "Forever" },
];

const TRANSFER_WINDOW_SEGMENTS = [
  { value: 5, label: "5 min" },
  { value: 15, label: "15 min" },
  { value: 30, label: "30 min" },
];

const CURRENCY_OPTIONS = ["IDR", "USD", "EUR", "GBP", "SGD", "JPY"];
const MONEY_SOURCE_TYPES: MoneySourceType[] = ["bank", "ewallet", "paylater", "credit_card"];

interface DowngradePromptState {
  pkg: string;
  appLabel: string;
  deleteExisting: boolean;
}

interface AddSourceDraftState {
  pkg: string;
  appLabel: string;
  types: MoneySourceType[];
}

interface RemoveAppPromptState {
  pkg: string;
  label: string;
  deleteSavedData: boolean;
}

interface AppPolicyRow {
  pkg: string;
  label: string;
  policy: AppNotificationPolicy;
  invalidPackage: boolean;
  isManual: boolean;
  isSensitive: boolean;
  isConfigured: boolean;
}

/** General (M-25..M-29, N-01..N-08): tray, updates, notifications, battery alerts, notification storage & Money setup. */
export function GeneralSection({ show }: { show: ShowFn }) {
  const appPrefs = useAppStore((s) => s.appPrefs);
  const setAppPrefs = useAppStore((s) => s.setAppPrefs);
  const devices = useAppStore((s) => s.devices);
  const notificationStorage = useAppStore((s) => s.notificationStorage);
  const setNotificationStorage = useAppStore((s) => s.setNotificationStorage);
  const vaultError = useAppStore((s) => s.vaultError);
  const setVaultError = useAppStore((s) => s.setVaultError);

  const notificationPollSecs = usePhoneHubStore((s) => s.notificationPollSecs);
  const setNotificationPollSecs = usePhoneHubStore((s) => s.setNotificationPollSecs);
  const notificationsBySerial = usePhoneHubStore((s) => s.notificationsBySerial);
  const sensitivePackages = usePhoneHubStore((s) => s.sensitivePackages);
  const setPackageSensitive = usePhoneHubStore((s) => s.setPackageSensitive);

  const [knownApps, setKnownApps] = useState<Array<{ package: string; label: string }>>([]);
  const [appSearch, setAppSearch] = useState("");
  const [otherAppsOpen, setOtherAppsOpen] = useState(false);
  const [addDisclosureOpen, setAddDisclosureOpen] = useState(false);
  const [newAppPkg, setNewAppPkg] = useState("");
  const [addAppError, setAddAppError] = useState<string | null>(null);
  const [addSourceDraft, setAddSourceDraft] = useState<AddSourceDraftState | null>(null);
  const [downgradePrompt, setDowngradePrompt] = useState<DowngradePromptState | null>(null);
  const [deleteAppPrompt, setDeleteAppPrompt] = useState<{ pkg: string; label: string } | null>(
    null,
  );
  const [removeAppPrompt, setRemoveAppPrompt] = useState<RemoveAppPromptState | null>(null);
  const [deleteAllOpen, setDeleteAllOpen] = useState(false);
  const [selectedCurrency, setSelectedCurrency] = useState("IDR");
  const [storageStatusMsg, setStorageStatusMsg] = useState<string | null>(null);

  const appSearchId = useId();
  const otherAppsPanelId = useId();
  const addAppPanelId = useId();
  const addAppInputId = useId();
  const addAppErrorId = useId();
  const windowInputId = useId();
  const incomeCurrencyId = useId();
  const incomeAmountId = useId();

  const update = (next: AppPrefs) => {
    setAppPrefs(next);
    void saveAppPrefs(next);
  };

  const commitStorage = (next: NotificationStorageSettings) => {
    setNotificationStorage(next);
    void saveNotificationStorage(next).catch(() => {});
    void phoneSetNotificationStorageConfig(next)
      .then((status) => {
        setVaultError(status.vaultError ?? null);
      })
      .catch((err) => {
        const msg = String(err);
        if (msg.includes("VAULT_DECRYPT_FAILED")) {
          setVaultError(msg);
        }
      });
  };

  const onlineSerial = useMemo(
    () => devices.find((d) => d.state === "device")?.serial ?? null,
    [devices],
  );

  useEffect(() => {
    if (!onlineSerial) return;
    let cancelled = false;
    void phoneListApps(onlineSerial, false)
      .then((apps) => {
        if (cancelled || !Array.isArray(apps)) return;
        setKnownApps(
          apps
            .filter((a) => !a.system)
            .map((a) => ({ package: a.package, label: a.label || a.package })),
        );
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [onlineSerial]);

  // Drop invalid per-app entries that have no saved vault data; keep those with saved data.
  useEffect(() => {
    let cancelled = false;
    void Promise.resolve(phoneGetSavedNotificationPackages?.())
      .then((savedPkgs) => {
        if (cancelled || !Array.isArray(savedPkgs)) return;
        const savedSet = new Set(savedPkgs);
        const currentStorage = useAppStore.getState().notificationStorage;
        const migrated = migrateInvalidPerAppEntries(currentStorage, savedSet);
        if (migrated.dropped.length > 0) {
          for (const droppedPkg of migrated.dropped) {
            setPackageSensitive(droppedPkg, false);
          }
          setNotificationStorage(migrated.settings);
          void saveNotificationStorage(migrated.settings).catch(() => {});
          void phoneSetNotificationStorageConfig(migrated.settings).catch(() => {});
        }
        for (const sensPkg of usePhoneHubStore.getState().sensitivePackages) {
          if (!isValidAndroidPackage(sensPkg) && !savedSet.has(sensPkg)) {
            setPackageSensitive(sensPkg, false);
          }
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [setNotificationStorage, setPackageSensitive]);

  // Close open dialogs on Escape
  useEffect(() => {
    if (!downgradePrompt && !deleteAppPrompt && !removeAppPrompt && !deleteAllOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setDowngradePrompt(null);
        setDeleteAppPrompt(null);
        setRemoveAppPrompt(null);
        setDeleteAllOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [downgradePrompt, deleteAppPrompt, removeAppPrompt, deleteAllOpen]);

  const updatePhase = useAppUpdateStore((s) => s.phase);
  const hasUpdate = useAppUpdateStore((s) => s.hasUpdate);
  const availableVersion = useAppUpdateStore((s) => s.availableVersion);
  const currentVersion = useAppUpdateStore((s) => s.currentVersion);
  const openUpdateModal = useAppUpdateStore((s) => s.openModal);
  const checkForUpdatesNow = useAppUpdateStore((s) => s.checkForUpdates);

  const row = (id: string, key: keyof AppPrefs) =>
    show(id) && (
      <SwitchRow
        label={entry(id).label}
        hint={entry(id).helper}
        checked={Boolean(appPrefs[key])}
        onChange={(v) => update({ ...appPrefs, [key]: v })}
      />
    );

  const devicePackageSet = useMemo(() => {
    const set = new Set<string>();
    for (const a of knownApps) set.add(a.package);
    for (const list of Object.values(notificationsBySerial)) {
      for (const n of list) set.add(n.package);
    }
    return set;
  }, [knownApps, notificationsBySerial]);

  // Merge configured apps, apps from live notifications, sensitive packages, and known user apps.
  const { configuredRows, otherRows } = useMemo(() => {
    const labelMap = new Map<string, string>();
    for (const a of knownApps) {
      labelMap.set(a.package, a.label);
    }
    for (const list of Object.values(notificationsBySerial)) {
      for (const n of list) {
        if (n.appLabel && n.appLabel !== n.package) {
          labelMap.set(n.package, n.appLabel);
        } else if (!labelMap.has(n.package)) {
          labelMap.set(n.package, n.package);
        }
      }
    }
    const sensitiveSet = new Set(sensitivePackages);
    const pkgs = new Set<string>([
      ...Object.keys(notificationStorage.perApp),
      ...sensitivePackages,
      ...labelMap.keys(),
    ]);
    const rows: AppPolicyRow[] = Array.from(pkgs).map((pkg) => {
      const policy = getAppPolicy(notificationStorage, pkg);
      const label = labelMap.get(pkg) ?? pkg;
      const invalidPackage = !isValidAndroidPackage(pkg);
      const isManual = !devicePackageSet.has(pkg) || invalidPackage;
      const isSensitive = policy.sensitive || sensitiveSet.has(pkg);
      const isConfigured =
        !isPolicyDefaultEmpty(policy) || sensitiveSet.has(pkg) || invalidPackage;
      return {
        pkg,
        label,
        policy,
        invalidPackage,
        isManual,
        isSensitive,
        isConfigured,
      };
    });
    const byLabel = (a: AppPolicyRow, b: AppPolicyRow) =>
      a.label.localeCompare(b.label) || a.pkg.localeCompare(b.pkg);
    return {
      configuredRows: rows.filter((r) => r.isConfigured).sort(byLabel),
      otherRows: rows.filter((r) => !r.isConfigured).sort(byLabel),
    };
  }, [knownApps, notificationsBySerial, notificationStorage, sensitivePackages, devicePackageSet]);

  const normalizedSearch = appSearch.trim().toLowerCase();
  const matchesAppSearch = (r: AppPolicyRow) =>
    !normalizedSearch ||
    r.label.toLowerCase().includes(normalizedSearch) ||
    r.pkg.toLowerCase().includes(normalizedSearch);
  const filteredConfiguredRows = configuredRows.filter(matchesAppSearch);
  const filteredOtherRows = otherRows.filter(matchesAppSearch);
  const totalAppCount = configuredRows.length + otherRows.length;
  const showOtherApps = otherAppsOpen || Boolean(normalizedSearch);

  const handleRetentionChange = (
    pkg: string,
    appLabel: string,
    prevPolicy: AppNotificationPolicy,
    nextRetention: RetentionPolicy,
  ) => {
    if (
      (prevPolicy.retention === "history" || prevPolicy.retention === "transactions") &&
      nextRetention === "none"
    ) {
      setDowngradePrompt({ pkg, appLabel, deleteExisting: true });
      return;
    }

    const next = withUpdatedAppPolicy(notificationStorage, pkg, (p) => ({
      ...p,
      retention: nextRetention,
      sensitive: nextRetention === "transactions" ? true : p.sensitive,
    }));
    if (nextRetention === "transactions") {
      setPackageSensitive(pkg, true);
    }
    commitStorage(next);
  };

  const confirmDowngradeToNone = async () => {
    if (!downgradePrompt) return;
    const { pkg, appLabel, deleteExisting } = downgradePrompt;
    setDowngradePrompt(null);
    const next = withUpdatedAppPolicy(notificationStorage, pkg, (p) => ({
      ...p,
      retention: "none",
    }));
    commitStorage(next);
    if (deleteExisting) {
      try {
        await phoneDeleteAppNotificationData(pkg, undefined, false);
        setStorageStatusMsg(`Cleared saved data for ${appLabel}.`);
      } catch {
        setStorageStatusMsg(`Switched ${appLabel} to Don't keep.`);
      }
    }
  };

  const handleConfirmDeleteApp = async () => {
    if (!deleteAppPrompt) return;
    const { pkg, label } = deleteAppPrompt;
    setDeleteAppPrompt(null);
    try {
      await phoneDeleteAppNotificationData(pkg, undefined, true);
      setStorageStatusMsg(`Deleted saved data for ${label}.`);
    } catch (e) {
      setStorageStatusMsg(String(e));
    }
  };

  const handleConfirmRemoveApp = async () => {
    if (!removeAppPrompt) return;
    const { pkg, label, deleteSavedData } = removeAppPrompt;
    setRemoveAppPrompt(null);
    if (addSourceDraft?.pkg === pkg) {
      setAddSourceDraft(null);
    }
    setPackageSensitive(pkg, false);
    const nextPerApp = { ...notificationStorage.perApp };
    delete nextPerApp[pkg];
    commitStorage({
      ...notificationStorage,
      perApp: nextPerApp,
    });
    if (deleteSavedData) {
      try {
        await phoneDeleteAppNotificationData(pkg, undefined, true);
        setStorageStatusMsg(`Removed ${label} and deleted saved data.`);
      } catch {
        setStorageStatusMsg(`Removed ${label} from list.`);
      }
    } else {
      setStorageStatusMsg(`Removed ${label} from list.`);
    }
  };

  const handleConfirmDeleteAll = async () => {
    setDeleteAllOpen(false);
    try {
      await phoneDeleteAllNotificationData();
      setVaultError(null);
      setStorageStatusMsg("Deleted all saved notification history and Money data.");
    } catch (e) {
      setStorageStatusMsg(String(e));
    }
  };

  const handleAddAppRow = () => {
    const trimmed = newAppPkg.trim();
    if (!trimmed) {
      setAddAppError("Enter an Android package ID (e.g. com.example.bank).");
      return;
    }
    if (!isValidAndroidPackage(trimmed)) {
      setAddAppError(
        "Enter a valid Android package ID (two or more dot-separated segments, e.g. com.example.bank).",
      );
      return;
    }
    const alreadyExists =
      configuredRows.some((r) => r.pkg === trimmed) ||
      otherRows.some((r) => r.pkg === trimmed);
    if (alreadyExists) {
      setAddAppError(`${trimmed} is already in your app list.`);
      return;
    }
    const next = withUpdatedAppPolicy(notificationStorage, trimmed, (p) => ({
      ...p,
      retention: "history",
      retentionDays: 30,
    }));
    commitStorage(next);
    setNewAppPkg("");
    setAddAppError(null);
    setAddDisclosureOpen(false);
  };

  const handleSaveNewSource = () => {
    if (!addSourceDraft || addSourceDraft.types.length === 0) return;
    const { pkg, appLabel, types } = addSourceDraft;
    const next = withUpdatedAppPolicy(notificationStorage, pkg, (prev) => {
      let sources = [...prev.moneySources];
      for (const type of types) {
        const id = createMoneySourceId(sources, type);
        const ruleTemplate = RULE_TEMPLATES_BY_TYPE[type][0].id;
        sources = [
          ...sources,
          {
            id,
            type,
            ruleTemplate,
            enabled: true,
            label: `${appLabel} ${MONEY_SOURCE_TYPE_LABELS[type]}`,
          },
        ];
      }
      return {
        ...prev,
        retention: prev.retention === "none" ? "transactions" : prev.retention,
        sensitive: true,
        moneySources: sources,
      };
    });
    setPackageSensitive(pkg, true);
    commitStorage(next);
    setAddSourceDraft(null);
  };

  const showTopPanel =
    show("M-25") || show("M-26") || show("M-27") || show("M-28") || show("M-29");
  const showStoragePanel =
    show("N-01") ||
    show("N-02") ||
    show("N-03") ||
    show("N-04") ||
    show("N-05") ||
    show("N-06") ||
    show("N-07") ||
    show("N-08");

  const currentThreshold =
    notificationStorage.largeIncomeThresholdByCurrency[selectedCurrency] ?? 0;

  const renderAppRow = ({
    pkg,
    label,
    policy,
    invalidPackage,
    isManual,
    isSensitive,
    isConfigured,
  }: AppPolicyRow) => {
    const isAddingSource = addSourceDraft?.pkg === pkg;
    return (
      <div
        key={pkg}
        data-app-policy-row={pkg}
        data-configured={isConfigured ? "true" : "false"}
        className="space-y-1.5 rounded-lg border border-ink-700 bg-ink-950 px-2.5 py-2"
      >
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-1.5">
              <span className="truncate text-xs font-semibold text-text-strong">{label}</span>
              {invalidPackage && (
                <span className="rounded border border-status-danger/50 bg-status-danger-bg px-1.5 py-0.5 text-[10px] font-medium text-status-danger">
                  Invalid package
                </span>
              )}
            </div>
            {label !== pkg && (
              <div className="truncate font-mono text-[10px] text-text-muted">{pkg}</div>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            <label className="sr-only" htmlFor={`ret-${pkg}`}>
              Retention for {label}
            </label>
            <select
              id={`ret-${pkg}`}
              aria-label={`Retention for ${label}`}
              value={policy.retention}
              onChange={(e) =>
                handleRetentionChange(pkg, label, policy, e.target.value as RetentionPolicy)
              }
              className={`${selectClass} !w-auto !py-1 !text-[11px]`}
            >
              <option value="none" className={optionClass}>
                Don&apos;t keep
              </option>
              <option value="history" className={optionClass}>
                Keep history
              </option>
              <option value="transactions" className={optionClass}>
                Transactions only
              </option>
            </select>

            {policy.retention === "history" && (
              <select
                aria-label={`History days for ${label}`}
                value={policy.retentionDays}
                onChange={(e) => {
                  const days = Number(e.target.value) as RetentionDays;
                  commitStorage(
                    withUpdatedAppPolicy(notificationStorage, pkg, (p) => ({
                      ...p,
                      retentionDays: days,
                    })),
                  );
                }}
                className={`${selectClass} !w-auto !py-1 !text-[11px]`}
              >
                <option value={7} className={optionClass}>
                  7 days
                </option>
                <option value={30} className={optionClass}>
                  30 days
                </option>
                <option value={90} className={optionClass}>
                  90 days
                </option>
              </select>
            )}

            <button
              type="button"
              role="switch"
              aria-checked={isSensitive}
              aria-label={`Sensitive ${label}`}
              onClick={() => {
                const nextSensitive = !isSensitive;
                setPackageSensitive(pkg, nextSensitive);
                commitStorage(
                  withUpdatedAppPolicy(notificationStorage, pkg, (p) => ({
                    ...p,
                    sensitive: nextSensitive,
                  })),
                );
              }}
              className={`btn-tactile cursor-pointer rounded-md border px-2 py-1 text-[11px] font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 ${
                isSensitive
                  ? "border-gold-400 bg-ink-800 text-gold-400"
                  : "border-ink-700 bg-ink-800 text-text hover:text-text-strong"
              }`}
            >
              Sensitive: {isSensitive ? "On" : "Off"}
            </button>

            {!isConfigured && !isAddingSource && (
              <button
                type="button"
                onClick={() =>
                  setAddSourceDraft({
                    pkg,
                    appLabel: label,
                    types: [],
                  })
                }
                className="btn-tactile inline-flex cursor-pointer items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] text-text hover:border-gold-400 hover:text-gold-400 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
              >
                <Plus aria-hidden="true" className="h-3 w-3" />
                Add money source
              </button>
            )}

            <button
              type="button"
              aria-label={`Delete saved data for ${label}`}
              onClick={() => setDeleteAppPrompt({ pkg, label })}
              className="btn-tactile inline-flex cursor-pointer items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] text-text hover:border-status-danger/60 hover:text-status-danger focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
            >
              <Trash2 aria-hidden="true" className="h-3 w-3" />
              Delete saved data
            </button>

            {isManual && (
              <button
                type="button"
                aria-label={`Remove ${label} from list`}
                onClick={() =>
                  setRemoveAppPrompt({ pkg, label, deleteSavedData: true })
                }
                className="btn-tactile inline-flex cursor-pointer items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] text-text hover:border-status-danger/60 hover:text-status-danger focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
              >
                <X aria-hidden="true" className="h-3 w-3" />
                Remove from list
              </button>
            )}
          </div>
        </div>

        {isConfigured &&
          policy.retention === "transactions" &&
          policy.moneySources.length === 0 && (
            <p className="text-[11px] text-gold-400">
              Add a Money source to track transactions from this app.
            </p>
          )}

        {policy.retention === "none" && policy.moneySources.some((s) => s.enabled) && (
          <div
            role="alert"
            className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-gold-400/40 bg-ink-900 px-2.5 py-1.5 text-[11px] text-gold-400"
          >
            <span className="flex items-center gap-1.5">
              <AlertTriangle aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />
              Set this app to Transactions only (or Keep history) so Money can count it.
            </span>
            <button
              type="button"
              onClick={() => handleRetentionChange(pkg, label, policy, "transactions")}
              className="btn-tactile cursor-pointer rounded border border-gold-400/50 bg-ink-800 px-2 py-0.5 text-[11px] font-medium text-gold-400 hover:bg-gold-400 hover:text-ink-950 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
            >
              Use Transactions only
            </button>
          </div>
        )}

        {(isConfigured || isAddingSource) && (
          <div className="space-y-1.5 border-t border-ink-700/70 pt-1.5">
            {isConfigured && (
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-[11px] font-medium text-text-muted">
                  Money sources ({policy.moneySources.length})
                </span>
                {!isAddingSource && (
                  <button
                    type="button"
                    onClick={() =>
                      setAddSourceDraft({
                        pkg,
                        appLabel: label,
                        types: [],
                      })
                    }
                    className="btn-tactile inline-flex cursor-pointer items-center gap-1 rounded border border-ink-700 bg-ink-800 px-2 py-0.5 text-[11px] text-text hover:border-gold-400 hover:text-gold-400 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                  >
                    <Plus aria-hidden="true" className="h-3 w-3" />
                    Add money source
                  </button>
                )}
              </div>
            )}

            {policy.moneySources.map((src) => (
              <div
                key={src.id}
                className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-ink-700 bg-ink-900 px-2.5 py-1 text-xs"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <span className="rounded border border-gold-400/40 bg-ink-950 px-1.5 py-0.5 text-[10px] font-medium text-gold-400">
                    {MONEY_SOURCE_TYPE_LABELS[src.type]}
                  </span>
                  <span className="text-[11px] text-text">
                    {formatRuleTemplateLabel(src.ruleTemplate)}
                  </span>
                  {src.label && <span className="text-[11px] text-text-muted">{src.label}</span>}
                </div>
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    role="switch"
                    aria-checked={src.enabled}
                    aria-label={`Enable source ${src.id}`}
                    onClick={() =>
                      commitStorage(
                        withUpdatedAppPolicy(notificationStorage, pkg, (p) => ({
                          ...p,
                          moneySources: p.moneySources.map((s) =>
                            s.id === src.id ? { ...s, enabled: !s.enabled } : s,
                          ),
                        })),
                      )
                    }
                    className={`cursor-pointer rounded border px-2 py-0.5 text-[10px] ${
                      src.enabled
                        ? "border-gold-400/50 bg-ink-800 text-gold-400"
                        : "border-ink-700 bg-ink-950 text-text-muted"
                    }`}
                  >
                    {src.enabled ? "Enabled" : "Disabled"}
                  </button>
                  <button
                    type="button"
                    aria-label={`Remove source ${src.id}`}
                    onClick={() =>
                      commitStorage(
                        withUpdatedAppPolicy(notificationStorage, pkg, (p) => ({
                          ...p,
                          moneySources: p.moneySources.filter((s) => s.id !== src.id),
                        })),
                      )
                    }
                    className="cursor-pointer rounded border border-ink-700 bg-ink-800 px-2 py-0.5 text-[10px] text-text hover:border-status-danger/60 hover:text-status-danger"
                  >
                    Remove
                  </button>
                </div>
              </div>
            ))}

            {isAddingSource && addSourceDraft && (
              <div className="space-y-2 rounded-md border border-gold-400/40 bg-ink-900 p-2.5 text-xs">
                <div>
                  <div className="mb-1.5 text-[11px] font-medium text-text-strong">
                    Select money source types
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {MONEY_SOURCE_TYPES.map((t) => {
                      const checked = addSourceDraft.types.includes(t);
                      const tplLabel = RULE_TEMPLATES_BY_TYPE[t][0].label;
                      return (
                        <label
                          key={t}
                          className={`inline-flex cursor-pointer items-center gap-1.5 rounded-md border px-2.5 py-1 text-[11px] transition-colors ${
                            checked
                              ? "border-gold-400 bg-ink-800 text-text-strong"
                              : "border-ink-700 bg-ink-950 text-text hover:border-ink-600"
                          }`}
                        >
                          <input
                            type="checkbox"
                            checked={checked}
                            onChange={(e) => {
                              const nextTypes = e.target.checked
                                ? [...addSourceDraft.types, t]
                                : addSourceDraft.types.filter((item) => item !== t);
                              setAddSourceDraft({
                                ...addSourceDraft,
                                types: nextTypes,
                              });
                            }}
                            className="accent-gold-400"
                          />
                          <span>{MONEY_SOURCE_TYPE_LABELS[t]}</span>
                          <span className="text-[10px] text-text-muted">({tplLabel})</span>
                        </label>
                      );
                    })}
                  </div>
                  <p className="mt-1.5 text-[11px] text-text-muted">
                    Many wallets also offer paylater — tick both if you use both.
                  </p>
                </div>

                <div className="flex items-center gap-1.5">
                  <button
                    type="button"
                    disabled={addSourceDraft.types.length === 0}
                    onClick={handleSaveNewSource}
                    className="btn-tactile cursor-pointer rounded-md bg-gold-400 px-2.5 py-1 text-xs font-semibold text-ink-950 hover:bg-gold-600 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    Save source
                  </button>
                  <button
                    type="button"
                    onClick={() => setAddSourceDraft(null)}
                    className="btn-tactile cursor-pointer rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs text-text hover:text-text-strong"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="space-y-4">
      {showTopPanel && (
        <Panel>
          {row("M-25", "minimizeToTrayOnClose")}
          {row("M-26", "checkUpdates")}
          {row("M-27", "desktopNotifications")}
          {row("M-28", "batteryAlerts")}
          {show("M-29") && (
            <div className="flex items-center justify-between gap-3 pt-2">
              <div>
                <div className="text-sm text-text-strong">{entry("M-29").label}</div>
                <p className="text-[11px] text-text-muted">
                  {updatePhase === "checking"
                    ? "Checking for updates..."
                    : hasUpdate
                      ? `v${availableVersion} available`
                      : `Current version v${currentVersion}`}
                </p>
              </div>
              {hasUpdate ? (
                <button
                  type="button"
                  onClick={openUpdateModal}
                  className="btn-tactile flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-gold-400/50 bg-ink-800 px-3 py-1.5 text-xs font-medium text-gold-400 hover:bg-ink-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                >
                  <Sparkles aria-hidden="true" className="h-3 w-3 text-gold-400" />
                  <span>View update</span>
                </button>
              ) : (
                <button
                  type="button"
                  disabled={updatePhase === "checking"}
                  onClick={() => void checkForUpdatesNow(true)}
                  className="btn-tactile flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 disabled:opacity-50"
                >
                  <RefreshCw
                    aria-hidden="true"
                    className={`h-3 w-3 ${
                      updatePhase === "checking" ? "animate-spin motion-reduce:animate-none" : ""
                    }`}
                  />
                  <span>Check now</span>
                </button>
              )}
            </div>
          )}
        </Panel>
      )}

      {showStoragePanel && (
        <Panel id="settings-phone-notifications-money" className="space-y-3">
          <div className="flex items-center justify-between gap-2 border-b border-ink-700 pb-2">
            <h3 className="text-sm font-semibold text-text-strong">
              Phone notifications &amp; Money
            </h3>
            {storageStatusMsg && (
              <span role="status" className="text-[11px] text-gold-400">
                {storageStatusMsg}
              </span>
            )}
          </div>

          {vaultError && (
            <div
              role="alert"
              className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-status-danger/40 bg-status-danger-bg p-3 text-xs text-status-danger"
            >
              <div className="flex items-center gap-2">
                <AlertTriangle className="h-4 w-4 shrink-0" />
                <span>
                  Unable to decrypt local vault for this Windows user. Delete &amp; reinitialize to
                  start fresh.
                </span>
              </div>
              <button
                type="button"
                onClick={() => void handleConfirmDeleteAll()}
                className="btn-tactile rounded-md border border-status-danger/50 bg-ink-950 px-2.5 py-1 text-xs font-semibold text-status-danger hover:bg-ink-900 cursor-pointer"
              >
                Delete &amp; reinitialize
              </button>
            </div>
          )}

          {show("N-01") && (
            <SwitchRow
              label={entry("N-01").label}
              hint={entry("N-01").helper}
              checked={notificationStorage.historyEnabled}
              onChange={(v) => commitStorage({ ...notificationStorage, historyEnabled: v })}
            />
          )}

          {show("N-02") && (
            <SegmentedControl
              label={entry("N-02").label}
              hint={entry("N-02").helper}
              segments={POLL_INTERVAL_SEGMENTS}
              value={
                POLL_INTERVAL_SEGMENTS.some((s) => s.value === notificationPollSecs)
                  ? notificationPollSecs
                  : null
              }
              customLabel={
                POLL_INTERVAL_SEGMENTS.some((s) => s.value === notificationPollSecs)
                  ? undefined
                  : `${notificationPollSecs} s`
              }
              onChange={(secs) => {
                setNotificationPollSecs(secs);
                void phoneHubSetSettings(secs).catch(() => {});
              }}
            />
          )}

          {show("N-03") && (
            <SwitchRow
              label={entry("N-03").label}
              hint={entry("N-03").helper}
              checked={notificationStorage.backgroundCapture}
              disabled={!notificationStorage.historyEnabled}
              disabledReason="Turn on notification saving above first."
              onChange={(v) => commitStorage({ ...notificationStorage, backgroundCapture: v })}
            />
          )}

          {show("N-04") && (
            <div className="space-y-2.5 border-t border-ink-700 pt-3">
              <div>
                <div className="text-sm text-text-strong">{entry("N-04").label}</div>
                <p className="text-[11px] leading-snug text-text-muted">{entry("N-04").helper}</p>
                <p className="mt-1 text-[11px] leading-snug text-text-muted">
                  Transactions only saves nothing yet. Money tracking is coming in a later update.
                </p>
              </div>

              <div className="relative">
                <label htmlFor={appSearchId} className="sr-only">
                  Search apps
                </label>
                <Search
                  aria-hidden="true"
                  className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-text-muted"
                />
                <input
                  id={appSearchId}
                  type="search"
                  placeholder="Search apps"
                  autoComplete="off"
                  value={appSearch}
                  onChange={(e) => setAppSearch(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      if (appSearch) {
                        e.preventDefault();
                        e.stopPropagation();
                        setAppSearch("");
                      }
                    } else if (e.key === "Enter") {
                      e.preventDefault();
                    }
                  }}
                  className="min-h-8 w-full rounded-lg border border-ink-700 bg-ink-950 py-1.5 pl-8 pr-8 text-xs text-text-strong placeholder:text-text-muted focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                />
                {appSearch && (
                  <button
                    type="button"
                    aria-label="Clear app search"
                    onClick={() => setAppSearch("")}
                    className="absolute right-1.5 top-1/2 flex h-6 w-6 -translate-y-1/2 cursor-pointer items-center justify-center rounded text-text hover:text-text-strong focus-visible:outline-2 focus-visible:outline-gold-400"
                  >
                    <X aria-hidden="true" className="h-3.5 w-3.5" />
                  </button>
                )}
              </div>

              {totalAppCount === 0 ? (
                <div className="rounded-lg border border-dashed border-ink-700 bg-ink-950 px-3 py-3 text-xs text-text-muted">
                  No apps listed yet. Connect your phone or use &ldquo;Add an app that isn&apos;t
                  listed&rdquo; below.
                </div>
              ) : filteredConfiguredRows.length === 0 && filteredOtherRows.length === 0 ? (
                <div className="flex items-center justify-between gap-2 rounded-lg border border-dashed border-ink-700 bg-ink-950 px-3 py-2.5 text-xs text-text-muted">
                  <span>No apps match &ldquo;{appSearch.trim()}&rdquo;.</span>
                  <button
                    type="button"
                    onClick={() => setAppSearch("")}
                    className="btn-tactile cursor-pointer rounded border border-ink-700 bg-ink-800 px-2 py-0.5 text-[11px] text-text hover:text-text-strong"
                  >
                    Clear
                  </button>
                </div>
              ) : (
                <div className="space-y-2">
                  {filteredConfiguredRows.length > 0 && (
                    <div className="space-y-1.5" data-app-section="configured">
                      {filteredConfiguredRows.map(renderAppRow)}
                    </div>
                  )}

                  {filteredOtherRows.length > 0 && (
                    <div className="space-y-1.5" data-app-section="other">
                      <button
                        type="button"
                        aria-expanded={showOtherApps}
                        aria-controls={otherAppsPanelId}
                        onClick={() => setOtherAppsOpen((v) => !v)}
                        className="flex min-h-8 w-full cursor-pointer items-center justify-between rounded-lg border border-ink-700 bg-ink-950 px-3 py-1.5 text-left text-xs font-medium text-text hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                      >
                        <span>Other apps ({filteredOtherRows.length})</span>
                        <ChevronDown
                          aria-hidden="true"
                          className={`h-3.5 w-3.5 text-text-muted transition-transform motion-reduce:transition-none ${
                            showOtherApps ? "rotate-180" : ""
                          }`}
                        />
                      </button>
                      {showOtherApps && (
                        <div id={otherAppsPanelId} className="space-y-1.5">
                          {filteredOtherRows.map(renderAppRow)}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              )}

              <div className="rounded-lg border border-ink-700 bg-ink-950 px-3 py-2">
                <button
                  type="button"
                  aria-expanded={addDisclosureOpen}
                  aria-controls={addAppPanelId}
                  onClick={() => {
                    setAddDisclosureOpen((v) => !v);
                    setAddAppError(null);
                  }}
                  className="flex min-h-7 w-full cursor-pointer items-center justify-between gap-2 text-left text-xs font-medium text-text hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                >
                  <span className="inline-flex items-center gap-1.5">
                    <Plus aria-hidden="true" className="h-3.5 w-3.5 text-gold-400" />
                    Add an app that isn&apos;t listed
                  </span>
                  <ChevronDown
                    aria-hidden="true"
                    className={`h-3.5 w-3.5 text-text-muted transition-transform motion-reduce:transition-none ${
                      addDisclosureOpen ? "rotate-180" : ""
                    }`}
                  />
                </button>
                {addDisclosureOpen && (
                  <div id={addAppPanelId} className="mt-2 space-y-1.5 border-t border-ink-700/70 pt-2">
                    <label
                      htmlFor={addAppInputId}
                      className="block text-[11px] text-text-muted"
                    >
                      Android package ID
                    </label>
                    <div className="flex flex-wrap items-center gap-2">
                      <input
                        id={addAppInputId}
                        type="text"
                        placeholder="e.g. com.example.bank"
                        value={newAppPkg}
                        aria-invalid={Boolean(addAppError)}
                        aria-describedby={addAppError ? addAppErrorId : undefined}
                        onChange={(e) => {
                          setNewAppPkg(e.target.value);
                          if (addAppError) setAddAppError(null);
                        }}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            e.preventDefault();
                            handleAddAppRow();
                          }
                        }}
                        className="min-w-[200px] flex-1 rounded-lg border border-ink-700 bg-ink-900 px-3 py-1.5 text-xs text-text-strong placeholder:text-text-muted focus:border-gold-400 focus:outline-none"
                      />
                      <button
                        type="button"
                        onClick={handleAddAppRow}
                        className="btn-tactile inline-flex min-h-8 cursor-pointer items-center gap-1 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1 text-xs font-medium text-text hover:border-gold-400 hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                      >
                        <Plus aria-hidden="true" className="h-3.5 w-3.5" />
                        Add app
                      </button>
                    </div>
                    {addAppError && (
                      <p id={addAppErrorId} role="alert" className="text-[11px] text-status-danger">
                        {addAppError}
                      </p>
                    )}
                  </div>
                )}
              </div>
            </div>
          )}

          {show("N-05") && (
            <div className="border-t border-ink-700 pt-2">
              <SegmentedControl
                label={entry("N-05").label}
                hint={entry("N-05").helper}
                segments={TX_RETENTION_SEGMENTS}
                value={notificationStorage.transactionRetention}
                onChange={(v) =>
                  commitStorage({ ...notificationStorage, transactionRetention: v })
                }
              />
            </div>
          )}

          {show("N-06") && (
            <div className="space-y-2 border-t border-ink-700 pt-2">
              <SegmentedControl
                label={entry("N-06").label}
                hint={entry("N-06").helper}
                segments={TRANSFER_WINDOW_SEGMENTS}
                value={
                  TRANSFER_WINDOW_SEGMENTS.some(
                    (s) => s.value === notificationStorage.transferMatchWindowMinutes,
                  )
                    ? notificationStorage.transferMatchWindowMinutes
                    : null
                }
                customLabel={
                  TRANSFER_WINDOW_SEGMENTS.some(
                    (s) => s.value === notificationStorage.transferMatchWindowMinutes,
                  )
                    ? undefined
                    : `${notificationStorage.transferMatchWindowMinutes} min`
                }
                onChange={(minutes) =>
                  commitStorage({
                    ...notificationStorage,
                    transferMatchWindowMinutes: clampTransferWindowMinutes(minutes),
                  })
                }
              />
              <div className="flex items-center gap-2">
                <label htmlFor={windowInputId} className="text-xs text-text-muted">
                  Custom minutes (1–60)
                </label>
                <input
                  id={windowInputId}
                  type="number"
                  min={1}
                  max={60}
                  value={notificationStorage.transferMatchWindowMinutes}
                  onChange={(e) =>
                    commitStorage({
                      ...notificationStorage,
                      transferMatchWindowMinutes: clampTransferWindowMinutes(
                        Number(e.target.value),
                      ),
                    })
                  }
                  className={`${selectClass} !w-24 !py-1`}
                />
              </div>
            </div>
          )}

          {show("N-07") && (
            <div className="space-y-1.5 border-t border-ink-700 pt-2">
              <div className="text-sm text-text-strong">{entry("N-07").label}</div>
              <p className="text-[11px] text-text-muted">{entry("N-07").helper}</p>
              <div className="flex flex-wrap items-center gap-2">
                <label htmlFor={incomeCurrencyId} className="sr-only">
                  Currency
                </label>
                <select
                  id={incomeCurrencyId}
                  value={selectedCurrency}
                  onChange={(e) => setSelectedCurrency(e.target.value)}
                  className={`${selectClass} !w-24 !py-1`}
                >
                  {CURRENCY_OPTIONS.map((c) => (
                    <option key={c} value={c} className={optionClass}>
                      {c}
                    </option>
                  ))}
                </select>
                <label htmlFor={incomeAmountId} className="sr-only">
                  Large income threshold amount
                </label>
                <input
                  id={incomeAmountId}
                  type="number"
                  min={0}
                  value={currentThreshold}
                  onChange={(e) => {
                    const val = Math.max(0, Math.round(Number(e.target.value) || 0));
                    const nextMap = { ...notificationStorage.largeIncomeThresholdByCurrency };
                    if (val === 0) {
                      delete nextMap[selectedCurrency];
                    } else {
                      nextMap[selectedCurrency] = val;
                    }
                    commitStorage({
                      ...notificationStorage,
                      largeIncomeThresholdByCurrency: nextMap,
                    });
                  }}
                  className={`${selectClass} !w-44 !py-1`}
                />
                <span className="text-[11px] text-text-muted">
                  {currentThreshold === 0 ? "Off" : `>= ${currentThreshold} ${selectedCurrency}`}
                </span>
              </div>
            </div>
          )}

          {show("N-08") && (
            <div className="flex flex-wrap items-center justify-between gap-3 border-t border-ink-700 pt-3">
              <div>
                <div className="text-sm text-text-strong">{entry("N-08").label}</div>
                <p className="text-[11px] text-text-muted">{entry("N-08").helper}</p>
              </div>
              <button
                type="button"
                onClick={() => setDeleteAllOpen(true)}
                className="btn-tactile flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-status-danger/50 bg-status-danger-bg px-3 py-1.5 text-xs font-semibold text-status-danger hover:border-status-danger focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
              >
                <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
                <span>Delete all saved data</span>
              </button>
            </div>
          )}
        </Panel>
      )}

      {/* Downgrade to "none" confirmation modal */}
      {downgradePrompt && (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="downgrade-modal-title"
          className="fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4"
        >
          <div className="w-full max-w-md space-y-3 rounded-xl border border-ink-700 bg-ink-900 p-4 shadow-elevated">
            <h4 id="downgrade-modal-title" className="text-sm font-semibold text-text-strong">
              Stop saving {downgradePrompt.appLabel}?
            </h4>
            <p className="text-xs text-text">
              Switching to Don&apos;t keep stops persisting new notifications for this app.
            </p>
            <label className="flex items-center gap-2 text-xs text-text-strong cursor-pointer">
              <input
                type="checkbox"
                checked={downgradePrompt.deleteExisting}
                onChange={(e) =>
                  setDowngradePrompt({
                    ...downgradePrompt,
                    deleteExisting: e.target.checked,
                  })
                }
                className="h-4 w-4 accent-gold-400"
              />
              Also delete existing saved data for {downgradePrompt.appLabel}?
            </label>
            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setDowngradePrompt(null)}
                className="btn-tactile rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs text-text hover:text-text-strong cursor-pointer"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void confirmDowngradeToNone()}
                className="btn-tactile rounded-lg bg-gold-400 px-3 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer"
              >
                Confirm
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Per-app delete confirmation modal */}
      {deleteAppPrompt && (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="delete-app-modal-title"
          className="fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4"
        >
          <div className="w-full max-w-md space-y-3 rounded-xl border border-ink-700 bg-ink-900 p-4 shadow-elevated">
            <h4 id="delete-app-modal-title" className="text-sm font-semibold text-text-strong">
              Delete saved data for {deleteAppPrompt.label}?
            </h4>
            <p className="text-xs text-text">
              This permanently removes saved notifications and any derived transaction records for{" "}
              <span className="font-mono">{deleteAppPrompt.pkg}</span> from this PC.
            </p>
            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setDeleteAppPrompt(null)}
                className="btn-tactile rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs text-text hover:text-text-strong cursor-pointer"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void handleConfirmDeleteApp()}
                className="btn-tactile rounded-lg border border-status-danger bg-status-danger-bg px-3 py-1.5 text-xs font-semibold text-status-danger cursor-pointer"
              >
                Delete app data
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Remove manually added app confirmation modal */}
      {removeAppPrompt && (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="remove-app-modal-title"
          className="fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4"
        >
          <div className="w-full max-w-md space-y-3 rounded-xl border border-ink-700 bg-ink-900 p-4 shadow-elevated">
            <h4 id="remove-app-modal-title" className="text-sm font-semibold text-text-strong">
              Remove {removeAppPrompt.label} from list?
            </h4>
            <p className="text-xs text-text">
              This resets the retention policy and Money sources for{" "}
              <span className="font-mono">{removeAppPrompt.pkg}</span> and removes it from your
              list.
            </p>
            <label className="flex cursor-pointer items-center gap-2 text-xs text-text-strong">
              <input
                type="checkbox"
                checked={removeAppPrompt.deleteSavedData}
                onChange={(e) =>
                  setRemoveAppPrompt({
                    ...removeAppPrompt,
                    deleteSavedData: e.target.checked,
                  })
                }
                className="h-4 w-4 accent-gold-400"
              />
              Also delete existing saved data for {removeAppPrompt.label}?
            </label>
            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setRemoveAppPrompt(null)}
                className="btn-tactile cursor-pointer rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs text-text hover:text-text-strong"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void handleConfirmRemoveApp()}
                className="btn-tactile cursor-pointer rounded-lg bg-gold-400 px-3 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600"
              >
                Remove
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Delete all confirmation modal */}
      {deleteAllOpen && (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="delete-all-modal-title"
          className="fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4"
        >
          <div className="w-full max-w-md space-y-3 rounded-xl border border-ink-700 bg-ink-900 p-4 shadow-elevated">
            <h4 id="delete-all-modal-title" className="text-sm font-semibold text-text-strong">
              Delete all saved notification data?
            </h4>
            <p className="text-xs text-text">
              Permanently deletes both encrypted vault files (notification history, Money
              transactions, and learned rules) from this PC. This cannot be undone.
            </p>
            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setDeleteAllOpen(false)}
                className="btn-tactile rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs text-text hover:text-text-strong cursor-pointer"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void handleConfirmDeleteAll()}
                className="btn-tactile rounded-lg border border-status-danger bg-status-danger-bg px-3 py-1.5 text-xs font-semibold text-status-danger cursor-pointer"
              >
                Delete everything
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
