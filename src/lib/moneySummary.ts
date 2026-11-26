import type {
  MoneySourceType,
  NotificationStorageSettings,
} from "@/lib/notificationStorage";
import type {
  LiabilityEvent,
  MoneyVaultSnapshot,
  StoredTransaction,
} from "@/types/phone";

export type MoneyPeriod = "today" | "week" | "month";

export type MoneyDirectionFilter =
  | "all"
  | "in"
  | "out"
  | "transfers"
  | "debt_payment"
  | "needs_review";

export interface PeriodTotals {
  inMinor: number;
  inCount: number;
  outMinor: number;
  outCount: number;
  netMinor: number;
  netCount: number;
}

export interface DailyInOutPoint {
  dateKey: string;
  label: string;
  fullDate: string;
  inMinor: number;
  outMinor: number;
  netMinor: number;
  ariaLabel: string;
}

export interface CategoryBreakdownItem {
  category: string;
  totalMinor: number;
  count: number;
  sharePct: number;
  ariaLabel: string;
}

export interface MoneySourceOption {
  compositeId: string; // `${package}::${sourceId}`
  packageName: string;
  sourceId: string;
  sourceType: MoneySourceType;
  label: string;
}

export type UnifiedMoneyRow =
  | {
      kind: "transaction";
      id: string;
      timestampMs: number;
      tx: StoredTransaction;
    }
  | {
      kind: "liability_event";
      id: string;
      timestampMs: number;
      event: LiabilityEvent;
    };

const SHORT_MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
] as const;

const ZERO_DECIMAL_CURRENCIES = new Set(["IDR", "JPY", "KRW", "VND"]);

export function countActiveMoneyApps(storage: NotificationStorageSettings): number {
  let count = 0;
  for (const policy of Object.values(storage.perApp)) {
    if (
      policy.retention !== "none" &&
      policy.moneySources.some((src) => src.enabled)
    ) {
      count += 1;
    }
  }
  return count;
}

export function isMoneySetupActive(storage: NotificationStorageSettings): boolean {
  return storage.historyEnabled && countActiveMoneyApps(storage) > 0;
}

export function getPeriodStartMs(period: MoneyPeriod, nowMs = Date.now()): number {
  const now = new Date(nowMs);
  if (period === "today") {
    return new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  }
  if (period === "week") {
    const day = now.getDay();
    const diffToMonday = (day + 6) % 7;
    return new Date(
      now.getFullYear(),
      now.getMonth(),
      now.getDate() - diffToMonday,
    ).getTime();
  }
  return new Date(now.getFullYear(), now.getMonth(), 1).getTime();
}

export function isIncludedInSpendingTotals(tx: StoredTransaction): boolean {
  if (tx.amountMinor <= 0) return false;
  return (
    tx.classification === "normal" || tx.classification === "possible_transfer"
  );
}

export function formatMinorAmount(
  amountMinor: number,
  currency: string,
  scale = 2,
  signMode?: "in" | "out" | "net" | "unsigned",
): string {
  const divisor = 10 ** Math.max(0, scale);
  const absMinor = Math.abs(amountMinor);
  const absValue = absMinor / divisor;
  const isWhole = divisor > 0 && absMinor % divisor === 0;
  const dropDecimals = isWhole && ZERO_DECIMAL_CURRENCIES.has(currency.toUpperCase());

  const formattedNum = new Intl.NumberFormat("en-US", {
    minimumFractionDigits: dropDecimals ? 0 : Math.min(2, scale),
    maximumFractionDigits: dropDecimals ? 0 : Math.min(2, scale),
  }).format(absValue);

  const cur = currency.toUpperCase();
  let prefix = "";
  if (signMode === "in") {
    prefix = "+";
  } else if (signMode === "out") {
    prefix = "-";
  } else if (signMode === "net") {
    if (amountMinor > 0) prefix = "+";
    else if (amountMinor < 0) prefix = "-";
  }
  return `${prefix}${cur} ${formattedNum}`;
}

export function pickPrimaryCurrency(
  snapshot: MoneyVaultSnapshot,
  preferredCurrency?: string | null,
): string | null {
  if (preferredCurrency && snapshot.currencies.includes(preferredCurrency)) {
    return preferredCurrency;
  }
  const counts = new Map<string, { count: number; latestMs: number }>();
  for (const tx of snapshot.transactions) {
    if (tx.classification === "ignored" || !tx.currency) continue;
    const prev = counts.get(tx.currency);
    if (!prev) {
      counts.set(tx.currency, { count: 1, latestMs: tx.timestampMs });
    } else {
      counts.set(tx.currency, {
        count: prev.count + 1,
        latestMs: Math.max(prev.latestMs, tx.timestampMs),
      });
    }
  }
  const candidates =
    counts.size > 0
      ? Array.from(counts.entries())
      : snapshot.currencies.map((c) => [c, { count: 0, latestMs: 0 }] as const);
  if (candidates.length === 0) return null;
  candidates.sort((a, b) => {
    if (b[1].count !== a[1].count) return b[1].count - a[1].count;
    if (b[1].latestMs !== a[1].latestMs) return b[1].latestMs - a[1].latestMs;
    return a[0].localeCompare(b[0]);
  });
  return candidates[0][0];
}

export function computePeriodTotals(
  transactions: StoredTransaction[],
  currency: string | null,
  period: MoneyPeriod,
  nowMs = Date.now(),
): PeriodTotals {
  if (!currency) {
    return {
      inMinor: 0,
      inCount: 0,
      outMinor: 0,
      outCount: 0,
      netMinor: 0,
      netCount: 0,
    };
  }
  const startMs = getPeriodStartMs(period, nowMs);
  let inMinor = 0;
  let inCount = 0;
  let outMinor = 0;
  let outCount = 0;

  for (const tx of transactions) {
    if (tx.currency !== currency || tx.timestampMs < startMs) continue;
    if (!isIncludedInSpendingTotals(tx)) continue;
    if (tx.direction === "in") {
      inMinor += tx.amountMinor;
      inCount += 1;
    } else {
      outMinor += tx.amountMinor;
      outCount += 1;
    }
  }

  return {
    inMinor,
    inCount,
    outMinor,
    outCount,
    netMinor: inMinor - outMinor,
    netCount: inCount + outCount,
  };
}

export function findLargeIncomes(
  transactions: StoredTransaction[],
  currency: string | null,
  period: MoneyPeriod,
  thresholdByCurrency: Record<string, number>,
  nowMs = Date.now(),
): StoredTransaction[] {
  if (!currency) return [];
  const threshold = thresholdByCurrency[currency] ?? 0;
  if (threshold <= 0) return [];
  const startMs = getPeriodStartMs(period, nowMs);

  return transactions
    .filter(
      (tx) =>
        tx.currency === currency &&
        tx.timestampMs >= startMs &&
        tx.direction === "in" &&
        isIncludedInSpendingTotals(tx) &&
        tx.amountMinor >= threshold,
    )
    .sort((a, b) => {
      if (b.amountMinor !== a.amountMinor) return b.amountMinor - a.amountMinor;
      return b.timestampMs - a.timestampMs;
    });
}

function toLocalDateKey(tsMs: number): string {
  const d = new Date(tsMs);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function computeDailyInOutSeries(
  transactions: StoredTransaction[],
  currency: string | null,
  nowMs = Date.now(),
  days = 7,
): DailyInOutPoint[] {
  const cur = currency ?? "IDR";
  const now = new Date(nowMs);
  const points: DailyInOutPoint[] = [];
  const byKey = new Map<string, { inMinor: number; outMinor: number }>();

  for (let offset = days - 1; offset >= 0; offset -= 1) {
    const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - offset);
    const key = toLocalDateKey(d.getTime());
    const label = `${SHORT_MONTHS[d.getMonth()]} ${d.getDate()}`;
    const fullDate = d.toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
    byKey.set(key, { inMinor: 0, outMinor: 0 });
    points.push({
      dateKey: key,
      label,
      fullDate,
      inMinor: 0,
      outMinor: 0,
      netMinor: 0,
      ariaLabel: "",
    });
  }

  if (currency) {
    for (const tx of transactions) {
      if (tx.currency !== currency || !isIncludedInSpendingTotals(tx)) continue;
      const bucket = byKey.get(toLocalDateKey(tx.timestampMs));
      if (!bucket) continue;
      if (tx.direction === "in") {
        bucket.inMinor += tx.amountMinor;
      } else {
        bucket.outMinor += tx.amountMinor;
      }
    }
  }

  for (const pt of points) {
    const bucket = byKey.get(pt.dateKey)!;
    pt.inMinor = bucket.inMinor;
    pt.outMinor = bucket.outMinor;
    pt.netMinor = bucket.inMinor - bucket.outMinor;
    pt.ariaLabel = `${pt.fullDate}: In ${formatMinorAmount(pt.inMinor, cur, 2, "unsigned")}, Out ${formatMinorAmount(pt.outMinor, cur, 2, "unsigned")}, Net ${formatMinorAmount(pt.netMinor, cur, 2, "net")}`;
  }

  return points;
}

export function formatDailyChartAriaSummary(
  points: DailyInOutPoint[],
  currency: string | null,
): string {
  const cur = currency ?? "IDR";
  const totalIn = points.reduce((acc, p) => acc + p.inMinor, 0);
  const totalOut = points.reduce((acc, p) => acc + p.outMinor, 0);
  return `Daily money in and out for the last ${points.length} days: total in ${formatMinorAmount(totalIn, cur, 2, "unsigned")}, total out ${formatMinorAmount(totalOut, cur, 2, "unsigned")}`;
}

export function computeCategoryBreakdown(
  transactions: StoredTransaction[],
  currency: string | null,
  period: MoneyPeriod,
  nowMs = Date.now(),
): CategoryBreakdownItem[] {
  if (!currency) return [];
  const startMs = getPeriodStartMs(period, nowMs);
  const grouped = new Map<string, { totalMinor: number; count: number }>();
  let grandTotal = 0;

  for (const tx of transactions) {
    if (
      tx.currency !== currency ||
      tx.timestampMs < startMs ||
      tx.direction !== "out" ||
      !isIncludedInSpendingTotals(tx)
    ) {
      continue;
    }
    const category = (tx.manualCategory || tx.category || "Uncategorized").trim() || "Uncategorized";
    const prev = grouped.get(category) ?? { totalMinor: 0, count: 0 };
    grouped.set(category, {
      totalMinor: prev.totalMinor + tx.amountMinor,
      count: prev.count + 1,
    });
    grandTotal += tx.amountMinor;
  }

  return Array.from(grouped.entries())
    .sort((a, b) => {
      if (b[1].totalMinor !== a[1].totalMinor) return b[1].totalMinor - a[1].totalMinor;
      return a[0].localeCompare(b[0]);
    })
    .map(([category, stats]) => {
      const sharePct = grandTotal > 0 ? Math.round((stats.totalMinor / grandTotal) * 100) : 0;
      return {
        category,
        totalMinor: stats.totalMinor,
        count: stats.count,
        sharePct,
        ariaLabel: `${category}: ${formatMinorAmount(stats.totalMinor, currency, 2, "unsigned")} (${sharePct}%)`,
      };
    });
}

export function formatCategoryAriaSummary(
  items: CategoryBreakdownItem[],
  currency: string | null,
): string {
  if (items.length === 0 || !currency) {
    return "Spending by category: no expenses in this period";
  }
  const top = items
    .slice(0, 3)
    .map((item) => `${item.category} ${formatMinorAmount(item.totalMinor, currency, 2, "unsigned")} (${item.sharePct}%)`)
    .join(", ");
  return `Spending by category (${items.length} categories): ${top}`;
}

export function buildSourceOptions(
  storage: NotificationStorageSettings,
  snapshot: MoneyVaultSnapshot,
): MoneySourceOption[] {
  const map = new Map<string, MoneySourceOption>();

  for (const [pkg, policy] of Object.entries(storage.perApp)) {
    for (const src of policy.moneySources) {
      if (!src.enabled) continue;
      const compositeId = `${pkg}::${src.id}`;
      map.set(compositeId, {
        compositeId,
        packageName: pkg,
        sourceId: src.id,
        sourceType: src.type,
        label: src.label || `${pkg} (${src.type})`,
      });
    }
  }

  for (const tx of snapshot.transactions) {
    const compositeId = `${tx.package}::${tx.sourceId}`;
    const existing = map.get(compositeId);
    if (!existing) {
      map.set(compositeId, {
        compositeId,
        packageName: tx.package,
        sourceId: tx.sourceId,
        sourceType: tx.sourceType,
        label: tx.appLabel ? `${tx.appLabel}` : compositeId,
      });
    } else if (existing.label.startsWith(tx.package) && tx.appLabel) {
      existing.label = tx.appLabel;
    }
  }

  for (const ev of snapshot.liabilityEvents) {
    const compositeId = `${ev.package}::${ev.sourceId}`;
    if (!map.has(compositeId)) {
      map.set(compositeId, {
        compositeId,
        packageName: ev.package,
        sourceId: ev.sourceId,
        sourceType: ev.sourceType,
        label: ev.appLabel || compositeId,
      });
    }
  }

  return Array.from(map.values()).sort((a, b) => a.label.localeCompare(b.label));
}

export function countReviewItems(
  transactions: StoredTransaction[],
  liabilityEvents: LiabilityEvent[],
  currency?: string | null,
): number {
  const txReviews = transactions.filter(
    (tx) =>
      tx.classification === "possible_transfer" &&
      (!currency || tx.currency === currency),
  ).length;
  const evReviews = liabilityEvents.filter(
    (ev) => !ev.duplicateOf && ev.status === "unmatched_needs_review",
  ).length;
  return txReviews + evReviews;
}

export function getSubAccountDisplayLabel(
  subAccount: string | null,
  nicknames: Record<string, string>,
  orderedUniqueSubAccounts: string[],
): string | null {
  if (!subAccount) return null;
  const nick = nicknames[subAccount];
  if (nick && nick.trim()) return nick.trim();
  const idx = orderedUniqueSubAccounts.indexOf(subAccount);
  const num = idx >= 0 ? idx + 1 : 1;
  return `Card #${num}`;
}

export function buildUnifiedFeedRows(
  snapshot: MoneyVaultSnapshot,
  activeCurrency: string | null,
  period: MoneyPeriod,
  directionFilter: MoneyDirectionFilter,
  sourceFilter: string,
  nowMs = Date.now(),
): UnifiedMoneyRow[] {
  const startMs = getPeriodStartMs(period, nowMs);
  const rows: UnifiedMoneyRow[] = [];

  for (const tx of snapshot.transactions) {
    if (tx.timestampMs < startMs) continue;
    if (activeCurrency && tx.currency !== activeCurrency) continue;
    if (tx.classification === "ignored") continue;
    const compositeSource = `${tx.package}::${tx.sourceId}`;
    if (sourceFilter !== "all" && compositeSource !== sourceFilter) continue;

    const matchesFilter =
      directionFilter === "all" ||
      (directionFilter === "in" && tx.direction === "in") ||
      (directionFilter === "out" && tx.direction === "out") ||
      (directionFilter === "transfers" && tx.classification === "transfer") ||
      (directionFilter === "debt_payment" &&
        (tx.classification === "debt_payment" ||
          tx.classification === "repayment_received")) ||
      (directionFilter === "needs_review" &&
        tx.classification === "possible_transfer");

    if (matchesFilter) {
      rows.push({
        kind: "transaction",
        id: tx.id,
        timestampMs: tx.timestampMs,
        tx,
      });
    }
  }

  for (const ev of snapshot.liabilityEvents) {
    if (ev.duplicateOf || ev.timestampMs < startMs) continue;
    if (ev.status !== "unmatched_needs_review") continue;
    const compositeSource = `${ev.package}::${ev.sourceId}`;
    if (sourceFilter !== "all" && compositeSource !== sourceFilter) continue;

    if (directionFilter === "all" || directionFilter === "needs_review") {
      rows.push({
        kind: "liability_event",
        id: ev.id,
        timestampMs: ev.timestampMs,
        event: ev,
      });
    }
  }

  rows.sort((a, b) => {
    if (b.timestampMs !== a.timestampMs) return b.timestampMs - a.timestampMs;
    return a.id.localeCompare(b.id);
  });
  return rows;
}

export function formatMoneyTimestamp(timestampMs: number, nowMs = Date.now()): string {
  if (!timestampMs || timestampMs <= 0) return "Unknown time";
  const d = new Date(timestampMs);
  const hhmm = d.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
  const todayStart = getPeriodStartMs("today", nowMs);
  const yesterdayStart = todayStart - 86_400_000;
  if (timestampMs >= todayStart) {
    return `Today · ${hhmm}`;
  }
  if (timestampMs >= yesterdayStart) {
    return `Yesterday · ${hhmm}`;
  }
  return `${SHORT_MONTHS[d.getMonth()]} ${d.getDate()} · ${hhmm}`;
}
