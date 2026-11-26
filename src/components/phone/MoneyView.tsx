import { useCallback, useEffect, useMemo, useState } from "react";
import { ArrowDownLeft, ArrowUpRight, AlertCircle, Settings, Wallet } from "lucide-react";
import {
  MONEY_SOURCE_TYPE_LABELS,
  type MoneySourceType,
} from "@/lib/notificationStorage";
import {
  buildSourceOptions,
  buildUnifiedFeedRows,
  computeCategoryBreakdown,
  computeDailyInOutSeries,
  computePeriodTotals,
  countActiveMoneyApps,
  countReviewItems,
  findLargeIncomes,
  formatCategoryAriaSummary,
  formatDailyChartAriaSummary,
  formatMinorAmount,
  formatMoneyTimestamp,
  getSubAccountDisplayLabel,
  isMoneySetupActive,
  pickPrimaryCurrency,
  type MoneyDirectionFilter,
  type MoneyPeriod,
} from "@/lib/moneySummary";
import { optionClass, selectClass } from "@/lib/styles";
import {
  onPhoneNotifications,
  phoneApplyTransactionCorrection,
  phoneGetTransactions,
  phoneResolveLiabilityEvent,
} from "@/services/phoneHubService";
import { useAppStore } from "@/store/useAppStore";
import type { MoneyVaultSnapshot, TxClassification } from "@/types/phone";
import { AppAvatar } from "./AppAvatar";

const INITIAL_VISIBLE_ROWS = 50;
const REFRESH_INTERVAL_MS = 15_000;

const EMPTY_SNAPSHOT: MoneyVaultSnapshot = {
  transactions: [],
  liabilityEvents: [],
  subAccountNicknames: {},
  currencies: [],
  ruleWarnings: [],
};

const PERIOD_OPTIONS: Array<{ id: MoneyPeriod; label: string }> = [
  { id: "today", label: "Today" },
  { id: "week", label: "This week" },
  { id: "month", label: "This month" },
];

const DIRECTION_FILTERS: Array<{ id: MoneyDirectionFilter; label: string }> = [
  { id: "all", label: "All" },
  { id: "in", label: "In" },
  { id: "out", label: "Out" },
  { id: "transfers", label: "Transfers" },
  { id: "debt_payment", label: "Debt payment" },
  { id: "needs_review", label: "Needs review" },
];

const CLASSIFICATION_LABELS: Partial<Record<TxClassification, string>> = {
  transfer: "Transfer between your accounts",
  possible_transfer: "Possible transfer",
  debt_payment: "Debt payment",
  repayment_received: "Repayment received",
};

function sourceTypeLabel(sourceType: MoneySourceType): string {
  return MONEY_SOURCE_TYPE_LABELS[sourceType] ?? sourceType;
}

export interface MoneyViewProps {
  hwSerial?: string | null;
  isOffline?: boolean;
  onOpenSettings?: () => void;
}

export function MoneyView({ hwSerial, isOffline = false, onOpenSettings }: MoneyViewProps) {
  const notificationStorage = useAppStore((s) => s.notificationStorage);
  const vaultError = useAppStore((s) => s.vaultError);

  const [snapshot, setSnapshot] = useState<MoneyVaultSnapshot>(EMPTY_SNAPSHOT);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [period, setPeriod] = useState<MoneyPeriod>("month");
  const [selectedCurrency, setSelectedCurrency] = useState<string | null>(null);
  const [directionFilter, setDirectionFilter] = useState<MoneyDirectionFilter>("all");
  const [sourceFilter, setSourceFilter] = useState<string>("all");
  const [visibleCount, setVisibleCount] = useState(INITIAL_VISIBLE_ROWS);
  const [busyRowId, setBusyRowId] = useState<string | null>(null);

  const activeAppsCount = useMemo(
    () => countActiveMoneyApps(notificationStorage),
    [notificationStorage],
  );
  const setupActive = isMoneySetupActive(notificationStorage);

  const loadSnapshot = useCallback(async () => {
    if (!setupActive) {
      setSnapshot(EMPTY_SNAPSHOT);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const next = await phoneGetTransactions({
        hwSerial: hwSerial ?? null,
      });
      setSnapshot(next ?? EMPTY_SNAPSHOT);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [setupActive, hwSerial]);

  useEffect(() => {
    void loadSnapshot();
  }, [loadSnapshot]);

  useEffect(() => {
    if (!setupActive) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void onPhoneNotifications(() => {
      if (!cancelled) {
        void loadSnapshot();
      }
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => {});

    const timer = setInterval(() => {
      void loadSnapshot();
    }, REFRESH_INTERVAL_MS);

    return () => {
      cancelled = true;
      clearInterval(timer);
      if (unlisten) unlisten();
    };
  }, [setupActive, loadSnapshot]);

  useEffect(() => {
    setVisibleCount(INITIAL_VISIBLE_ROWS);
  }, [period, selectedCurrency, directionFilter, sourceFilter]);

  const activeCurrency = useMemo(
    () => pickPrimaryCurrency(snapshot, selectedCurrency),
    [snapshot, selectedCurrency],
  );

  const nowMs = Date.now();

  const periodTotals = useMemo(
    () => computePeriodTotals(snapshot.transactions, activeCurrency, period, nowMs),
    [snapshot.transactions, activeCurrency, period, nowMs],
  );

  const largeIncomes = useMemo(
    () =>
      findLargeIncomes(
        snapshot.transactions,
        activeCurrency,
        period,
        notificationStorage.largeIncomeThresholdByCurrency,
        nowMs,
      ),
    [
      snapshot.transactions,
      activeCurrency,
      period,
      notificationStorage.largeIncomeThresholdByCurrency,
      nowMs,
    ],
  );
  const largeIncomeIds = useMemo(
    () => new Set(largeIncomes.map((tx) => tx.id)),
    [largeIncomes],
  );

  const reviewCount = useMemo(
    () => countReviewItems(snapshot.transactions, snapshot.liabilityEvents, activeCurrency),
    [snapshot.transactions, snapshot.liabilityEvents, activeCurrency],
  );

  const dailySeries = useMemo(
    () => computeDailyInOutSeries(snapshot.transactions, activeCurrency, nowMs, 7),
    [snapshot.transactions, activeCurrency, nowMs],
  );
  const maxDailyMinor = useMemo(
    () => Math.max(1, ...dailySeries.map((d) => Math.max(d.inMinor, d.outMinor))),
    [dailySeries],
  );

  const categories = useMemo(
    () => computeCategoryBreakdown(snapshot.transactions, activeCurrency, period, nowMs),
    [snapshot.transactions, activeCurrency, period, nowMs],
  );

  const sourceOptions = useMemo(
    () => buildSourceOptions(notificationStorage, snapshot),
    [notificationStorage, snapshot],
  );

  const orderedSubAccounts = useMemo(() => {
    const set = new Set<string>();
    for (const tx of snapshot.transactions) {
      if (tx.subAccount) set.add(tx.subAccount);
    }
    for (const ev of snapshot.liabilityEvents) {
      if (ev.subAccount) set.add(ev.subAccount);
    }
    return Array.from(set).sort();
  }, [snapshot.transactions, snapshot.liabilityEvents]);

  const feedRows = useMemo(
    () =>
      buildUnifiedFeedRows(
        snapshot,
        activeCurrency,
        period,
        directionFilter,
        sourceFilter,
        nowMs,
      ),
    [snapshot, activeCurrency, period, directionFilter, sourceFilter, nowMs],
  );

  const visibleRows = feedRows.slice(0, visibleCount);
  const hasMoreRows = feedRows.length > visibleRows.length;

  const handleTxCorrection = async (
    txId: string,
    classification: TxClassification,
  ) => {
    setBusyRowId(txId);
    setError(null);
    try {
      await phoneApplyTransactionCorrection(txId, classification, null, false);
      await loadSnapshot();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyRowId(null);
    }
  };

  const handleDismissBillPaid = async (eventId: string) => {
    setBusyRowId(eventId);
    setError(null);
    try {
      await phoneResolveLiabilityEvent(eventId, null, true);
      await loadSnapshot();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyRowId(null);
    }
  };

  const disclaimerFooter = (
    <footer className="rounded-lg border border-ink-700 bg-ink-900 px-3 py-2 text-[11px] text-text-muted">
      Estimate only — calculated from captured phone notifications while connected. Cash, missed
      notifications, and unrecognized formats are excluded.
    </footer>
  );

  if (!setupActive) {
    return (
      <div className="flex flex-1 flex-col justify-between gap-4 p-4">
        <div className="flex flex-1 flex-col items-center justify-center rounded-xl border border-ink-700 bg-ink-900 p-8 text-center">
          <Wallet className="mb-3 h-9 w-9 text-gold-400" aria-hidden="true" />
          <h2 className="text-sm font-semibold text-text-strong">
            Set up Money tracking
          </h2>
          <p className="mt-1.5 max-w-md text-xs text-text-muted">
            {!notificationStorage.historyEnabled
              ? "Notification persistence is currently turned off. Enable saving notifications and mark at least one banking or wallet app as a Money source to track spending and income locally."
              : "No apps have an active Money source yet. Enable a Bank, E-wallet, Paylater, or Credit card source in Settings to start tracking."}
          </p>
          {onOpenSettings && (
            <button
              type="button"
              onClick={onOpenSettings}
              className="btn-tactile mt-4 inline-flex items-center gap-1.5 rounded-lg border border-gold-400 bg-gold-400 px-3.5 py-2 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer"
            >
              <Settings className="h-3.5 w-3.5" aria-hidden="true" />
              Open Settings → General
            </button>
          )}
        </div>
        {disclaimerFooter}
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col gap-4 p-4">
      {/* Header */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-text-strong">Money</h2>
          <p className="text-xs text-text-muted">
            Estimate from banking and wallet notifications · {activeAppsCount}{" "}
            {activeAppsCount === 1 ? "app" : "apps"}
            {isOffline ? " · Offline" : ""}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {snapshot.currencies.length > 1 && (
            <div
              role="group"
              aria-label="Currency"
              className="inline-flex rounded-lg border border-ink-700 bg-ink-900 p-0.5"
            >
              {snapshot.currencies.map((cur) => {
                const active = cur === activeCurrency;
                return (
                  <button
                    key={cur}
                    type="button"
                    aria-pressed={active}
                    onClick={() => setSelectedCurrency(cur)}
                    className={`btn-tactile rounded-md px-2.5 py-1 font-mono text-[11px] font-semibold cursor-pointer ${
                      active
                        ? "bg-ink-800 text-gold-400"
                        : "text-text-muted hover:text-text-strong"
                    }`}
                  >
                    {cur}
                  </button>
                );
              })}
            </div>
          )}

          <div
            role="group"
            aria-label="Period"
            className="inline-flex rounded-lg border border-ink-700 bg-ink-900 p-0.5"
          >
            {PERIOD_OPTIONS.map((opt) => {
              const active = period === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  aria-pressed={active}
                  onClick={() => setPeriod(opt.id)}
                  className={`btn-tactile rounded-md px-2.5 py-1 text-xs font-medium cursor-pointer ${
                    active
                      ? "bg-ink-800 text-gold-400"
                      : "text-text-muted hover:text-text-strong"
                  }`}
                >
                  {opt.label}
                </button>
              );
            })}
          </div>
        </div>
      </div>

      {(vaultError || error) && (
        <div
          role="alert"
          className="flex items-center justify-between gap-2 rounded-lg border border-status-danger/40 bg-status-danger-bg px-3 py-2 text-xs text-status-danger"
        >
          <span>{vaultError || error}</span>
          {onOpenSettings && vaultError && (
            <button
              type="button"
              onClick={onOpenSettings}
              className="underline cursor-pointer"
            >
              Settings
            </button>
          )}
        </div>
      )}

      {/* Three summary cards */}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div className="rounded-xl border border-ink-700 bg-ink-900 p-3.5">
          <div className="flex items-center justify-between text-[11px] text-text-muted">
            <span>Money in</span>
            <ArrowDownLeft className="h-3.5 w-3.5 text-status-success" aria-hidden="true" />
          </div>
          <div className="mt-1 font-mono text-lg font-semibold text-status-success">
            {activeCurrency
              ? formatMinorAmount(periodTotals.inMinor, activeCurrency, 2, "unsigned")
              : "—"}
          </div>
          <div className="mt-0.5 text-[11px] text-text-muted">
            {periodTotals.inCount}{" "}
            {periodTotals.inCount === 1 ? "transaction" : "transactions"}
          </div>
        </div>

        <div className="rounded-xl border border-ink-700 bg-ink-900 p-3.5">
          <div className="flex items-center justify-between text-[11px] text-text-muted">
            <span>Money out</span>
            <ArrowUpRight className="h-3.5 w-3.5 text-status-danger" aria-hidden="true" />
          </div>
          <div className="mt-1 font-mono text-lg font-semibold text-status-danger">
            {activeCurrency
              ? formatMinorAmount(periodTotals.outMinor, activeCurrency, 2, "unsigned")
              : "—"}
          </div>
          <div className="mt-0.5 text-[11px] text-text-muted">
            {periodTotals.outCount}{" "}
            {periodTotals.outCount === 1 ? "transaction" : "transactions"}
          </div>
        </div>

        <div className="rounded-xl border border-ink-700 bg-ink-900 p-3.5">
          <div className="flex items-center justify-between text-[11px] text-text-muted">
            <span>Net</span>
          </div>
          <div
            className={`mt-1 font-mono text-lg font-semibold ${
              periodTotals.netMinor > 0
                ? "text-status-success"
                : periodTotals.netMinor < 0
                  ? "text-status-danger"
                  : "text-text-strong"
            }`}
          >
            {activeCurrency
              ? formatMinorAmount(periodTotals.netMinor, activeCurrency, 2, "net")
              : "—"}
          </div>
          <div className="mt-0.5 text-[11px] text-text-muted">
            {periodTotals.netCount}{" "}
            {periodTotals.netCount === 1 ? "transaction" : "transactions"}
          </div>
        </div>
      </div>

      {/* Large income strip */}
      {largeIncomes.length > 0 && activeCurrency && (
        <section
          aria-label="Large income"
          className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-gold-400/50 bg-ink-900 px-3.5 py-2.5"
        >
          <div className="flex items-center gap-2">
            <span className="rounded bg-gold-400 px-2 py-0.5 text-[10px] font-bold uppercase text-ink-950">
              Large income
            </span>
            <span className="text-xs font-medium text-text-strong">
              {largeIncomes[0].appLabel || largeIncomes[0].package} ·{" "}
              {largeIncomes[0].manualCategory || largeIncomes[0].category}
              {largeIncomes.length > 1 ? ` (+${largeIncomes.length - 1} more)` : ""}
            </span>
          </div>
          <span className="font-mono text-xs font-semibold text-gold-400">
            {formatMinorAmount(
              largeIncomes[0].amountMinor,
              largeIncomes[0].currency,
              largeIncomes[0].scale,
              "in",
            )}
          </span>
        </section>
      )}

      {/* Review pill */}
      {reviewCount > 0 && (
        <div className="flex items-center justify-between gap-2">
          <button
            type="button"
            aria-pressed={directionFilter === "needs_review"}
            onClick={() =>
              setDirectionFilter((prev) =>
                prev === "needs_review" ? "all" : "needs_review",
              )
            }
            className={`btn-tactile inline-flex items-center gap-2 rounded-full border px-3 py-1 text-xs font-medium cursor-pointer ${
              directionFilter === "needs_review"
                ? "border-gold-400 bg-ink-800 text-gold-400"
                : "border-status-warning/50 bg-status-warning-bg text-status-warning hover:border-gold-400"
            }`}
          >
            <AlertCircle className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
            <span>
              {reviewCount} {reviewCount === 1 ? "item" : "items"} to review
            </span>
          </button>
        </div>
      )}

      {/* Daily chart + Spending by category */}
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <section
          aria-label={formatDailyChartAriaSummary(dailySeries, activeCurrency)}
          className="rounded-xl border border-ink-700 bg-ink-900 p-3.5"
        >
          <div className="mb-3 flex items-center justify-between gap-2">
            <div>
              <h3 className="text-xs font-semibold text-text-strong">
                Daily in and out
              </h3>
              <p className="text-[11px] text-text-muted">Last 7 days</p>
            </div>
            <div className="flex items-center gap-3 text-[11px] text-text-muted">
              <span className="inline-flex items-center gap-1">
                <span className="h-2 w-2 rounded-sm bg-status-success" aria-hidden="true" />
                In
              </span>
              <span className="inline-flex items-center gap-1">
                <span className="h-2 w-2 rounded-sm bg-status-danger" aria-hidden="true" />
                Out
              </span>
            </div>
          </div>

          <div className="grid grid-cols-7 gap-2 pt-1">
            {dailySeries.map((pt) => {
              const inPct = pt.inMinor > 0 ? Math.max(8, Math.round((pt.inMinor / maxDailyMinor) * 100)) : 0;
              const outPct = pt.outMinor > 0 ? Math.max(8, Math.round((pt.outMinor / maxDailyMinor) * 100)) : 0;
              return (
                <div
                  key={pt.dateKey}
                  tabIndex={0}
                  aria-label={pt.ariaLabel}
                  title={pt.ariaLabel}
                  className="flex flex-col items-center gap-1 rounded-md p-1 focus-visible:outline-2 focus-visible:outline-gold-400"
                >
                  <div className="flex h-12 w-full items-end justify-center">
                    {inPct > 0 && (
                      <div
                        style={{ height: `${inPct}%` }}
                        className="w-3 rounded-t-sm bg-status-success transition-[height] duration-150 motion-reduce:transition-none"
                      />
                    )}
                  </div>
                  <div className="h-px w-full bg-ink-700" aria-hidden="true" />
                  <div className="flex h-12 w-full items-start justify-center">
                    {outPct > 0 && (
                      <div
                        style={{ height: `${outPct}%` }}
                        className="w-3 rounded-b-sm bg-status-danger transition-[height] duration-150 motion-reduce:transition-none"
                      />
                    )}
                  </div>
                  <span className="text-[10px] text-text-muted">{pt.label}</span>
                </div>
              );
            })}
          </div>
        </section>

        <section
          aria-label={formatCategoryAriaSummary(categories, activeCurrency)}
          className="rounded-xl border border-ink-700 bg-ink-900 p-3.5"
        >
          <div className="mb-3 flex items-center justify-between gap-2">
            <h3 className="text-xs font-semibold text-text-strong">
              Spending by category
            </h3>
            <span className="text-[11px] text-text-muted">
              {PERIOD_OPTIONS.find((p) => p.id === period)?.label}
            </span>
          </div>

          {categories.length === 0 ? (
            <p className="py-6 text-center text-xs text-text-muted">
              No categorized expenses in this period.
            </p>
          ) : (
            <ul className="space-y-2.5">
              {categories.slice(0, 6).map((cat) => (
                <li key={cat.category} aria-label={cat.ariaLabel} className="space-y-1">
                  <div className="flex items-center justify-between gap-2 text-xs">
                    <span className="truncate font-medium text-text-strong">
                      {cat.category}
                    </span>
                    <span className="shrink-0 font-mono text-[11px] text-text">
                      {activeCurrency
                        ? formatMinorAmount(cat.totalMinor, activeCurrency, 2, "unsigned")
                        : cat.totalMinor}{" "}
                      <span className="text-text-muted">({cat.sharePct}%)</span>
                    </span>
                  </div>
                  <div className="h-1.5 w-full overflow-hidden rounded-full bg-ink-800">
                    <div
                      style={{ width: `${Math.max(4, cat.sharePct)}%` }}
                      className="h-full rounded-full bg-gold-400 transition-[width] duration-150 motion-reduce:transition-none"
                    />
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>

      {/* Filters & Transactions list */}
      <section className="rounded-xl border border-ink-700 bg-ink-900 p-3.5">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <div role="group" aria-label="Filter transactions" className="flex flex-wrap gap-1.5">
            {DIRECTION_FILTERS.map((f) => {
              const active = directionFilter === f.id;
              return (
                <button
                  key={f.id}
                  type="button"
                  aria-pressed={active}
                  onClick={() => setDirectionFilter(f.id)}
                  className={`btn-tactile rounded-full border px-2.5 py-1 text-[11px] font-medium cursor-pointer ${
                    active
                      ? "border-gold-400 bg-ink-800 text-gold-400"
                      : "border-ink-700 bg-ink-950 text-text-muted hover:text-text-strong"
                  }`}
                >
                  {f.label}
                </button>
              );
            })}
          </div>

          <div className="flex items-center gap-2">
            <label htmlFor="money-source-filter" className="sr-only">
              Filter by source
            </label>
            <select
              id="money-source-filter"
              aria-label="Filter by source"
              value={sourceFilter}
              onChange={(e) => setSourceFilter(e.target.value)}
              className={`${selectClass} !w-auto min-w-[160px] !py-1 text-xs`}
            >
              <option value="all" className={optionClass}>
                All sources
              </option>
              {sourceOptions.map((opt) => (
                <option
                  key={opt.compositeId}
                  value={opt.compositeId}
                  className={optionClass}
                >
                  {opt.label} · {sourceTypeLabel(opt.sourceType)}
                </option>
              ))}
            </select>
          </div>
        </div>

        {loading && feedRows.length === 0 ? (
          <p className="py-8 text-center text-xs text-text-muted">
            Loading transactions…
          </p>
        ) : visibleRows.length === 0 ? (
          <p className="py-8 text-center text-xs text-text-muted">
            No matching transactions in this period.
          </p>
        ) : (
          <div className="divide-y divide-ink-700">
            {visibleRows.map((row) => {
              if (row.kind === "liability_event") {
                const ev = row.event;
                const subLabel = getSubAccountDisplayLabel(
                  ev.subAccount,
                  snapshot.subAccountNicknames,
                  orderedSubAccounts,
                );
                const busy = busyRowId === ev.id;
                return (
                  <div
                    key={ev.id}
                    className="flex flex-wrap items-center justify-between gap-3 py-2.5"
                  >
                    <div className="flex min-w-0 items-center gap-2.5">
                      <AppAvatar
                        label={ev.appLabel || ev.package}
                        packageName={ev.package}
                      />
                      <div className="min-w-0">
                        <div className="flex flex-wrap items-center gap-1.5">
                          <span className="text-xs font-medium text-text-strong">
                            Bill paid (amount unknown)
                          </span>
                          <span className="rounded border border-status-warning/40 bg-status-warning-bg px-1.5 py-0.5 text-[10px] font-medium text-status-warning">
                            Bill paid (amount unknown)
                          </span>
                        </div>
                        <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[11px] text-text-muted">
                          <span>{formatMoneyTimestamp(ev.timestampMs, nowMs)}</span>
                          <span>·</span>
                          <span>{ev.appLabel || ev.package}</span>
                          <span className="rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] text-text">
                            {sourceTypeLabel(ev.sourceType)}
                          </span>
                          {subLabel && (
                            <span className="rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] text-text">
                              {subLabel}
                            </span>
                          )}
                        </div>
                      </div>
                    </div>

                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs text-text-muted">
                        Amount unknown
                      </span>
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void handleDismissBillPaid(ev.id)}
                        className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] font-medium text-text hover:border-gold-400 hover:text-text-strong disabled:opacity-50 cursor-pointer"
                      >
                        Dismiss
                      </button>
                    </div>
                  </div>
                );
              }

              const tx = row.tx;
              const subLabel = getSubAccountDisplayLabel(
                tx.subAccount,
                snapshot.subAccountNicknames,
                orderedSubAccounts,
              );
              const classBadge = CLASSIFICATION_LABELS[tx.classification];
              const isLarge = largeIncomeIds.has(tx.id);
              const busy = busyRowId === tx.id;
              const displayCategory =
                tx.manualCategory || tx.category || "Uncategorized";

              return (
                <div
                  key={tx.id}
                  className="flex flex-wrap items-center justify-between gap-3 py-2.5"
                >
                  <div className="flex min-w-0 items-center gap-2.5">
                    <AppAvatar
                      label={tx.appLabel || tx.package}
                      packageName={tx.package}
                    />
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span className="text-xs font-medium text-text-strong">
                          {displayCategory}
                        </span>
                        {classBadge && (
                          <span
                            className={`rounded border px-1.5 py-0.5 text-[10px] font-medium ${
                              tx.classification === "possible_transfer"
                                ? "border-status-warning/40 bg-status-warning-bg text-status-warning"
                                : "border-ink-700 bg-ink-800 text-text"
                            }`}
                          >
                            {classBadge}
                          </span>
                        )}
                        {isLarge && (
                          <span className="rounded bg-gold-400 px-1.5 py-0.5 text-[10px] font-bold text-ink-950">
                            Large income
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[11px] text-text-muted">
                        <span>{formatMoneyTimestamp(tx.timestampMs, nowMs)}</span>
                        <span>·</span>
                        <span>{tx.appLabel || tx.package}</span>
                        <span className="rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] text-text">
                          {sourceTypeLabel(tx.sourceType)}
                        </span>
                        {subLabel && (
                          <span className="rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] text-text">
                            {subLabel}
                          </span>
                        )}
                      </div>
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center gap-2">
                    <span
                      className={`font-mono text-xs font-semibold ${
                        tx.classification === "transfer" ||
                        tx.classification === "debt_payment" ||
                        tx.classification === "repayment_received"
                          ? "text-text"
                          : tx.direction === "in"
                            ? "text-status-success"
                            : "text-status-danger"
                      }`}
                    >
                      {formatMinorAmount(
                        tx.amountMinor,
                        tx.currency,
                        tx.scale,
                        tx.direction,
                      )}
                    </span>

                    {tx.classification === "possible_transfer" && (
                      <div className="flex flex-wrap items-center gap-1">
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void handleTxCorrection(tx.id, "transfer")}
                          className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] font-medium text-text hover:border-gold-400 hover:text-text-strong disabled:opacity-50 cursor-pointer"
                        >
                          Mark as transfer
                        </button>
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void handleTxCorrection(tx.id, "debt_payment")}
                          className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] font-medium text-text hover:border-gold-400 hover:text-text-strong disabled:opacity-50 cursor-pointer"
                        >
                          Mark as debt payment
                        </button>
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void handleTxCorrection(tx.id, "normal")}
                          className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] font-medium text-text hover:border-gold-400 hover:text-text-strong disabled:opacity-50 cursor-pointer"
                        >
                          Keep as expense
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {hasMoreRows && (
          <div className="mt-3 flex justify-center">
            <button
              type="button"
              onClick={() => setVisibleCount((n) => n + INITIAL_VISIBLE_ROWS)}
              className="btn-tactile rounded-lg border border-ink-700 bg-ink-800 px-3.5 py-1.5 text-xs font-medium text-text hover:border-gold-400 hover:text-text-strong cursor-pointer"
            >
              Show more ({feedRows.length - visibleRows.length} remaining)
            </button>
          </div>
        )}
      </section>

      {disclaimerFooter}
    </div>
  );
}
