import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_NOTIFICATION_STORAGE,
  type NotificationStorageSettings,
} from "@/lib/notificationStorage";
import { useAppStore } from "@/store/useAppStore";
import type {
  LiabilityEvent,
  MoneyVaultSnapshot,
  StoredTransaction,
} from "@/types/phone";

const mockPhoneGetTransactions = vi.fn<
  (...args: unknown[]) => Promise<MoneyVaultSnapshot>
>();
const mockPhoneApplyTransactionCorrection = vi.fn<
  (...args: unknown[]) => Promise<unknown>
>();
const mockPhoneResolveLiabilityEvent = vi.fn<
  (...args: unknown[]) => Promise<unknown>
>();
const mockOnPhoneNotifications = vi.fn<
  (...args: unknown[]) => Promise<() => void>
>(() => Promise.resolve(() => {}));

vi.mock("@/services/phoneHubService", () => ({
  phoneGetTransactions: (...args: unknown[]) => mockPhoneGetTransactions(...args),
  phoneApplyTransactionCorrection: (...args: unknown[]) =>
    mockPhoneApplyTransactionCorrection(...args),
  phoneResolveLiabilityEvent: (...args: unknown[]) =>
    mockPhoneResolveLiabilityEvent(...args),
  onPhoneNotifications: (...args: unknown[]) => mockOnPhoneNotifications(...args),
}));

import { MoneyView } from "../MoneyView";

function makeTx(overrides: Partial<StoredTransaction> = {}): StoredTransaction {
  return {
    id: "tx_default",
    dedupeId: "HW1::k::1",
    hwSerial: "HW1",
    package: "com.example.bank",
    sourceId: "src_bank_1",
    appLabel: "Example Bank",
    sourceType: "bank",
    subAccount: null,
    direction: "out",
    amountMinor: 120_000_00,
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
    timestampMs: Date.now() - 10_000,
    ruleId: "r1",
    ...overrides,
  };
}

function makeBillPaid(overrides: Partial<LiabilityEvent> = {}): LiabilityEvent {
  return {
    id: "ev_bill_1",
    dedupeId: "HW1::k_bill::2",
    hwSerial: "HW1",
    package: "com.example.shop",
    sourceId: "src_cc_1",
    appLabel: "Example Shop",
    sourceType: "credit_card",
    eventType: "bill_paid",
    subAccount: "sa_hash_1",
    status: "unmatched_needs_review",
    linkedTxId: null,
    resolvedAmountMinor: null,
    resolvedCurrency: null,
    scale: null,
    manualLocked: false,
    duplicateOf: null,
    timestampMs: Date.now() - 5_000,
    ruleId: "r_bill",
    ...overrides,
  };
}

function enabledStorage(
  thresholds: Record<string, number> = {},
): NotificationStorageSettings {
  return {
    ...DEFAULT_NOTIFICATION_STORAGE,
    historyEnabled: true,
    largeIncomeThresholdByCurrency: thresholds,
    perApp: {
      "com.example.bank": {
        retention: "transactions",
        retentionDays: 30,
        sensitive: true,
        moneySources: [
          {
            id: "src_bank_1",
            type: "bank",
            ruleTemplate: "generic_bank",
            enabled: true,
            label: "Primary Bank",
          },
        ],
      },
      "com.example.shop": {
        retention: "transactions",
        retentionDays: 30,
        sensitive: true,
        moneySources: [
          {
            id: "src_cc_1",
            type: "credit_card",
            ruleTemplate: "generic_credit_card",
            enabled: true,
            label: "Shop Card",
          },
        ],
      },
    },
  };
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("MoneyView (SPEC-33 Phase 3)", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    mockPhoneGetTransactions.mockReset();
    mockPhoneApplyTransactionCorrection.mockReset();
    mockPhoneResolveLiabilityEvent.mockReset();
    mockOnPhoneNotifications.mockClear();

    useAppStore.getState().setVaultError(null);
    useAppStore.getState().setNotificationStorage({
      ...DEFAULT_NOTIFICATION_STORAGE,
      largeIncomeThresholdByCurrency: {},
      perApp: {},
    });

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  it("renders setup empty state when history is off and navigates to Settings", async () => {
    const onOpenSettings = vi.fn();
    await act(async () => {
      root.render(<MoneyView hwSerial="HW1" onOpenSettings={onOpenSettings} />);
    });
    await flush();

    expect(container.textContent).toContain("Set up Money tracking");
    const btn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Open Settings"),
    );
    expect(btn).toBeTruthy();

    await act(async () => {
      btn!.click();
    });
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
    expect(container.textContent).not.toContain("Export CSV");
  });

  it("excludes transfers and debt payments from totals, switches currencies, shows large income strip, and exposes accessible summaries", async () => {
    useAppStore.getState().setNotificationStorage(enabledStorage({ IDR: 1_000_000_00 }));

    const snap: MoneyVaultSnapshot = {
      transactions: [
        makeTx({
          id: "in_big",
          direction: "in",
          amountMinor: 2_500_000_00,
          currency: "IDR",
          category: "Salary",
        }),
        makeTx({
          id: "out_groceries",
          direction: "out",
          amountMinor: 120_000_00,
          currency: "IDR",
          category: "Groceries",
        }),
        makeTx({
          id: "out_tr",
          direction: "out",
          amountMinor: 500_000_00,
          currency: "IDR",
          classification: "transfer",
          category: "Transfer",
        }),
        makeTx({
          id: "out_debt",
          direction: "out",
          amountMinor: 300_000_00,
          currency: "IDR",
          classification: "debt_payment",
          category: "Debt payment",
        }),
        makeTx({
          id: "usd_coffee",
          direction: "out",
          amountMinor: 15_50,
          currency: "USD",
          category: "Coffee",
        }),
      ],
      liabilityEvents: [],
      subAccountNicknames: {},
      currencies: ["IDR", "USD"],
      ruleWarnings: [],
    };
    mockPhoneGetTransactions.mockResolvedValue(snap);

    await act(async () => {
      root.render(<MoneyView hwSerial="HW1" />);
    });
    await flush();

    // IDR totals exclude transfer (500k) and debt_payment (300k)
    expect(container.textContent).toContain("IDR 2,500,000");
    expect(container.textContent).toContain("IDR 120,000");
    expect(container.textContent).toContain("+IDR 2,380,000");

    // Large income strip visible
    const largeSection = container.querySelector('section[aria-label="Large income"]');
    expect(largeSection).toBeTruthy();
    expect(largeSection?.textContent).toContain("Large income");

    // Accessible chart and category summaries
    const chartSection = container.querySelector('section[aria-label^="Daily money in and out"]');
    expect(chartSection).toBeTruthy();
    const catSection = container.querySelector('section[aria-label^="Spending by category"]');
    expect(catSection).toBeTruthy();

    // Disclaimer present, no Export CSV button or Cash-flow toggle
    expect(container.textContent).toContain("Estimate only — calculated from captured phone notifications");
    expect(container.textContent).not.toContain("Export CSV");
    expect(container.textContent).not.toContain("Cash flow");

    // Switch currency to USD
    const usdBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "USD",
    );
    expect(usdBtn).toBeTruthy();
    await act(async () => {
      usdBtn!.click();
    });
    await flush();

    expect(container.textContent).toContain("USD 15.50");
    expect(container.querySelector('section[aria-label="Large income"]')).toBeNull();
  });

  it("shows review pill count, filters to review items, and runs every review action updating the UI", async () => {
    useAppStore.getState().setNotificationStorage(enabledStorage());

    const vaTx1 = makeTx({
      id: "va_1",
      direction: "out",
      amountMinor: 200_000_00,
      classification: "possible_transfer",
      transferHint: "virtual_account",
      category: "Transfer",
    });
    const vaTx2 = makeTx({
      id: "va_2",
      direction: "out",
      amountMinor: 100_000_00,
      classification: "possible_transfer",
      transferHint: "virtual_account",
      category: "Transfer",
    });
    const vaTx3 = makeTx({
      id: "va_3",
      direction: "out",
      amountMinor: 50_000_00,
      classification: "possible_transfer",
      transferHint: "virtual_account",
      category: "Transfer",
    });
    const billEv = makeBillPaid({ id: "ev_1" });

    let currentSnap: MoneyVaultSnapshot = {
      transactions: [vaTx1, vaTx2, vaTx3],
      liabilityEvents: [billEv],
      subAccountNicknames: {},
      currencies: ["IDR"],
      ruleWarnings: [],
    };
    mockPhoneGetTransactions.mockImplementation(async () => currentSnap);
    mockPhoneApplyTransactionCorrection.mockImplementation(
      async (txId: unknown, cls: unknown) => {
        currentSnap = {
          ...currentSnap,
          transactions: currentSnap.transactions.map((t) =>
            t.id === txId ? { ...t, classification: cls as StoredTransaction["classification"] } : t,
          ),
        };
        return { updatedCount: 1 };
      },
    );
    mockPhoneResolveLiabilityEvent.mockImplementation(async (evId: unknown) => {
      currentSnap = {
        ...currentSnap,
        liabilityEvents: currentSnap.liabilityEvents.map((e) =>
          e.id === evId ? { ...e, status: "ignored" } : e,
        ),
      };
      return {};
    });

    await act(async () => {
      root.render(<MoneyView hwSerial="HW1" />);
    });
    await flush();

    // 3 possible transfers + 1 unmatched bill_paid = 4 items to review
    const reviewPill = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("4 items to review"),
    );
    expect(reviewPill).toBeTruthy();

    // Click review pill to filter list
    await act(async () => {
      reviewPill!.click();
    });
    await flush();
    expect(reviewPill?.getAttribute("aria-pressed")).toBe("true");

    // 1) Mark as transfer (va_1) -> leaves review queue and totals drop by 200k (350k -> 150k)
    const markTransferBtns = Array.from(container.querySelectorAll("button")).filter(
      (b) => b.textContent?.trim() === "Mark as transfer",
    );
    await act(async () => {
      markTransferBtns[0].click();
    });
    await flush();
    expect(mockPhoneApplyTransactionCorrection).toHaveBeenCalledWith(
      "va_1",
      "transfer",
      null,
      false,
    );
    expect(container.textContent).toContain("3 items to review");
    expect(container.textContent).toContain("IDR 150,000");

    // 2) Mark as debt payment (va_2) -> leaves review queue and totals drop by 100k (150k -> 50k)
    const markDebtBtns = Array.from(container.querySelectorAll("button")).filter(
      (b) => b.textContent?.trim() === "Mark as debt payment",
    );
    await act(async () => {
      markDebtBtns[0].click();
    });
    await flush();
    expect(mockPhoneApplyTransactionCorrection).toHaveBeenCalledWith(
      "va_2",
      "debt_payment",
      null,
      false,
    );
    expect(container.textContent).toContain("2 items to review");
    expect(container.textContent).toContain("IDR 50,000");

    // 3) Keep as expense (va_3) -> leaves review queue, stays in totals (50k)
    const keepExpenseBtns = Array.from(container.querySelectorAll("button")).filter(
      (b) => b.textContent?.trim() === "Keep as expense",
    );
    await act(async () => {
      keepExpenseBtns[0].click();
    });
    await flush();
    expect(mockPhoneApplyTransactionCorrection).toHaveBeenCalledWith(
      "va_3",
      "normal",
      null,
      false,
    );
    expect(container.textContent).toContain("1 item to review");
    expect(container.textContent).toContain("IDR 50,000");

    // 4) Dismiss bill_paid -> queue empties
    const dismissBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Dismiss",
    );
    expect(dismissBtn).toBeTruthy();
    await act(async () => {
      dismissBtn!.click();
    });
    await flush();
    expect(mockPhoneResolveLiabilityEvent).toHaveBeenCalledWith("ev_1", null, true);
    expect(container.textContent).not.toContain("items to review");
    expect(container.textContent).not.toContain("item to review");
  });

  it("paginates rows after 50 items with a Show more button", async () => {
    useAppStore.getState().setNotificationStorage(enabledStorage());
    const txs = Array.from({ length: 55 }, (_, i) =>
      makeTx({
        id: `tx_page_${i}`,
        category: `Item ${i + 1}`,
        amountMinor: 10_000_00,
        timestampMs: Date.now() - (i + 1) * 1000,
      }),
    );
    mockPhoneGetTransactions.mockResolvedValue({
      transactions: txs,
      liabilityEvents: [],
      subAccountNicknames: {},
      currencies: ["IDR"],
      ruleWarnings: [],
    });

    await act(async () => {
      root.render(<MoneyView hwSerial="HW1" />);
    });
    await flush();

    expect(container.textContent).toContain("Item 50");
    expect(container.textContent).not.toContain("Item 55");
    const showMoreBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Show more (5 remaining)"),
    );
    expect(showMoreBtn).toBeTruthy();

    await act(async () => {
      showMoreBtn!.click();
    });
    await flush();
    expect(container.textContent).toContain("Item 55");
  });
});
