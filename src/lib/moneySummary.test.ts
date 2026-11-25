import { describe, expect, it } from "vitest";
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
  getSubAccountDisplayLabel,
  pickPrimaryCurrency,
} from "./moneySummary";
import { DEFAULT_NOTIFICATION_STORAGE, type NotificationStorageSettings } from "./notificationStorage";
import type { LiabilityEvent, MoneyVaultSnapshot, StoredTransaction } from "@/types/phone";

function makeTx(overrides: Partial<StoredTransaction> = {}): StoredTransaction {
  return {
    id: "tx_1",
    dedupeId: "HW1::k1::100",
    hwSerial: "HW1",
    package: "com.example.bank",
    sourceId: "src_bank_1",
    appLabel: "Example Bank",
    sourceType: "bank",
    subAccount: null,
    direction: "out",
    amountMinor: 10_000_00,
    scale: 2,
    currency: "IDR",
    category: "Groceries",
    classification: "normal",
    manualClassification: null,
    manualCategory: null,
    transferPairId: null,
    linkedLiabilityEventId: null,
    linkedLiabilitySourceId: null,
    linkedSubAccount: null,
    transferHint: null,
    similarityHash: null,
    statementDueMinor: null,
    statementDueUnix: null,
    timestampMs: new Date(2026, 10, 26, 10, 0, 0).getTime(),
    ruleId: "r1",
    ...overrides,
  };
}

function makeBillPaid(overrides: Partial<LiabilityEvent> = {}): LiabilityEvent {
  return {
    id: "le_1",
    dedupeId: "HW1::k2::100",
    hwSerial: "HW1",
    package: "com.example.shop",
    sourceId: "src_cc_1",
    appLabel: "Example Shop",
    sourceType: "credit_card",
    eventType: "bill_paid",
    subAccount: "sa_abc",
    status: "unmatched_needs_review",
    linkedTxId: null,
    resolvedAmountMinor: null,
    resolvedCurrency: null,
    scale: null,
    manualLocked: false,
    duplicateOf: null,
    timestampMs: new Date(2026, 10, 26, 11, 0, 0).getTime(),
    ruleId: "r_bill",
    ...overrides,
  };
}

describe("moneySummary (SPEC-33 Phase 3)", () => {
  const nowMs = new Date(2026, 10, 26, 15, 0, 0).getTime();

  it("excludes transfers, debt payments, repayments received, and ignored rows while counting CC/paylater purchases", () => {
    const txs: StoredTransaction[] = [
      makeTx({ id: "in1", direction: "in", amountMinor: 500_000_00, category: "Income" }),
      makeTx({ id: "out_bank", direction: "out", amountMinor: 100_000_00, category: "Food" }),
      makeTx({
        id: "out_cc",
        package: "com.example.shop",
        sourceType: "credit_card",
        direction: "out",
        amountMinor: 50_000_00,
        category: "Shopping",
      }),
      makeTx({
        id: "out_pl",
        package: "com.example.wallet",
        sourceType: "paylater",
        direction: "out",
        amountMinor: 25_000_00,
        category: "Bills",
      }),
      makeTx({
        id: "tr_out",
        direction: "out",
        amountMinor: 200_000_00,
        classification: "transfer",
      }),
      makeTx({
        id: "tr_in",
        direction: "in",
        amountMinor: 200_000_00,
        classification: "transfer",
      }),
      makeTx({
        id: "debt_out",
        direction: "out",
        amountMinor: 150_000_00,
        classification: "debt_payment",
      }),
      makeTx({
        id: "rep_in",
        sourceType: "credit_card",
        direction: "in",
        amountMinor: 150_000_00,
        classification: "repayment_received",
      }),
      makeTx({
        id: "ign",
        direction: "out",
        amountMinor: 999_000_00,
        classification: "ignored",
      }),
    ];

    const totals = computePeriodTotals(txs, "IDR", "today", nowMs);
    expect(totals.inMinor).toBe(500_000_00);
    expect(totals.inCount).toBe(1);
    expect(totals.outMinor).toBe(175_000_00);
    expect(totals.outCount).toBe(3);
    expect(totals.netMinor).toBe(325_000_00);
  });

  it("keeps currencies separate and picks primary currency by frequency", () => {
    const snap: MoneyVaultSnapshot = {
      transactions: [
        makeTx({ id: "u1", currency: "USD", amountMinor: 25_00 }),
        makeTx({ id: "i1", currency: "IDR", amountMinor: 100_000_00 }),
        makeTx({ id: "i2", currency: "IDR", amountMinor: 200_000_00 }),
      ],
      liabilityEvents: [],
      subAccountNicknames: {},
      currencies: ["IDR", "USD"],
      ruleWarnings: [],
    };
    expect(pickPrimaryCurrency(snap)).toBe("IDR");
    expect(pickPrimaryCurrency(snap, "USD")).toBe("USD");
    expect(computePeriodTotals(snap.transactions, "USD", "month", nowMs).outMinor).toBe(25_00);
    expect(formatMinorAmount(2550, "USD", 2, "out")).toBe("-USD 25.50");
    expect(formatMinorAmount(120_000_00, "IDR", 2, "in")).toBe("+IDR 120,000");
  });

  it("finds large incomes at or above currency threshold and computes chart + category summaries", () => {
    const dayAgo = nowMs - 86_400_000;
    const txs = [
      makeTx({ id: "in_big", direction: "in", amountMinor: 2_000_000_00, category: "Salary" }),
      makeTx({ id: "in_small", direction: "in", amountMinor: 500_000_00, category: "Gift" }),
      makeTx({ id: "o1", direction: "out", amountMinor: 300_000_00, category: "Groceries" }),
      makeTx({ id: "o2", direction: "out", amountMinor: 100_000_00, category: "Transit", timestampMs: dayAgo }),
      makeTx({ id: "o_tr", direction: "out", amountMinor: 900_000_00, classification: "transfer", category: "Transfer" }),
    ];

    const large = findLargeIncomes(txs, "IDR", "month", { IDR: 2_000_000_00 }, nowMs);
    expect(large.map((t) => t.id)).toEqual(["in_big"]);

    const series = computeDailyInOutSeries(txs, "IDR", nowMs, 7);
    expect(series).toHaveLength(7);
    expect(series[6].inMinor).toBe(2_500_000_00);
    expect(series[6].outMinor).toBe(300_000_00);
    expect(series[5].outMinor).toBe(100_000_00);
    expect(formatDailyChartAriaSummary(series, "IDR")).toContain("IDR 2,500,000");
    expect(formatDailyChartAriaSummary(series, "IDR")).toContain("IDR 400,000");

    const cats = computeCategoryBreakdown(txs, "IDR", "month", nowMs);
    expect(cats.map((c) => [c.category, c.sharePct])).toEqual([
      ["Groceries", 75],
      ["Transit", 25],
    ]);
    expect(formatCategoryAriaSummary(cats, "IDR")).toContain("Groceries IDR 300,000 (75%)");
  });

  it("counts review items, distinguishes repeated sourceIds across packages, and formats sub_account labels", () => {
    const txs = [
      makeTx({ id: "pt1", package: "com.example.a", sourceId: "src_1", classification: "possible_transfer" }),
      makeTx({ id: "n1", package: "com.example.b", sourceId: "src_1", classification: "normal" }),
    ];
    const evs = [
      makeBillPaid({ id: "ev1", status: "unmatched_needs_review" }),
      makeBillPaid({ id: "ev2", status: "ignored", duplicateOf: "ev1" }),
    ];
    const snap: MoneyVaultSnapshot = {
      transactions: txs,
      liabilityEvents: evs,
      subAccountNicknames: { sa_1: "Everyday Card" },
      currencies: ["IDR"],
      ruleWarnings: [],
    };
    const storage: NotificationStorageSettings = {
      ...DEFAULT_NOTIFICATION_STORAGE,
      historyEnabled: true,
      perApp: {
        "com.example.a": {
          retention: "transactions",
          retentionDays: 30,
          sensitive: true,
          moneySources: [{ id: "src_1", type: "bank", ruleTemplate: "generic_bank", enabled: true }],
        },
        "com.example.b": {
          retention: "transactions",
          retentionDays: 30,
          sensitive: true,
          moneySources: [{ id: "src_1", type: "ewallet", ruleTemplate: "generic_ewallet", enabled: true }],
        },
      },
    };
    expect(countActiveMoneyApps(storage)).toBe(2);
    expect(countReviewItems(txs, evs, "IDR")).toBe(2);

    const opts = buildSourceOptions(storage, snap);
    expect(opts.map((o) => o.compositeId)).toContain("com.example.a::src_1");
    expect(opts.map((o) => o.compositeId)).toContain("com.example.b::src_1");

    const rows = buildUnifiedFeedRows(snap, "IDR", "month", "needs_review", "all", nowMs);
    expect(rows).toHaveLength(2);

    expect(getSubAccountDisplayLabel("sa_1", snap.subAccountNicknames, ["sa_1", "sa_2"])).toBe("Everyday Card");
    expect(getSubAccountDisplayLabel("sa_2", snap.subAccountNicknames, ["sa_1", "sa_2"])).toBe("Card #2");
  });
});
