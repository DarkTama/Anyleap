import { describe, expect, it } from "vitest";
import {
  DEFAULT_NOTIFICATION_STORAGE,
  clampTransferWindowMinutes,
  createMoneySourceId,
  formatRetentionBadge,
  isValidAndroidPackage,
  migrateInvalidPerAppEntries,
  normalizeNotificationStorage,
  withUpdatedAppPolicy,
  type NotificationStorageSettings,
} from "./notificationStorage";
import { loadNotificationStorageFrom, type KeyValueStore } from "./settingsStore";

function memoryStore(initial: Record<string, unknown> = {}): KeyValueStore {
  const data = new Map<string, unknown>(Object.entries(initial));
  return {
    async get<T>(key: string) {
      return data.get(key) as T | undefined;
    },
    async set(key: string, value: unknown) {
      data.set(key, value);
    },
    async save() {},
  };
}

describe("notificationStorage (SPEC-33 Phase 2)", () => {
  it("defaults to opt-in off, background capture off, 1y transaction retention, and 15 min window", () => {
    expect(DEFAULT_NOTIFICATION_STORAGE.historyEnabled).toBe(false);
    expect(DEFAULT_NOTIFICATION_STORAGE.backgroundCapture).toBe(false);
    expect(DEFAULT_NOTIFICATION_STORAGE.transactionRetention).toBe("1y");
    expect(DEFAULT_NOTIFICATION_STORAGE.transferMatchWindowMinutes).toBe(15);
    expect(DEFAULT_NOTIFICATION_STORAGE.perApp).toEqual({});
  });

  it("normalizes invalid settings and supports 0..n Money sources per app", () => {
    const normalized = normalizeNotificationStorage({
      version: 2,
      historyEnabled: true,
      backgroundCapture: true,
      transactionRetention: "2y",
      transferMatchWindowMinutes: 120,
      largeIncomeThresholdByCurrency: { idr: 1_000_000, bad: -5 },
      perApp: {
        "com.example.shop": {
          retention: "transactions",
          retentionDays: 999,
          sensitive: true,
          moneySources: [
            {
              id: "src_1",
              type: "credit_card",
              ruleTemplate: "cobranded_card_in_shop_app",
              enabled: true,
            },
            {
              id: "src_2",
              type: "paylater",
              ruleTemplate: "generic_paylater",
              enabled: false,
            },
            {
              id: "src_1", // duplicate id ignored
              type: "bank",
              ruleTemplate: "generic_bank",
              enabled: true,
            },
            {
              type: "invalid_type",
            },
          ],
        },
      },
    });

    expect(normalized.historyEnabled).toBe(true);
    expect(normalized.backgroundCapture).toBe(true);
    expect(normalized.transactionRetention).toBe("2y");
    expect(normalized.transferMatchWindowMinutes).toBe(60);
    expect(normalized.largeIncomeThresholdByCurrency).toEqual({ IDR: 1_000_000 });

    const shop = normalized.perApp["com.example.shop"];
    expect(shop.retention).toBe("transactions");
    expect(shop.retentionDays).toBe(30);
    expect(shop.moneySources).toHaveLength(2);
    expect(shop.moneySources[0].ruleTemplate).toBe("cobranded_card_in_shop_app");
    expect(shop.moneySources[1].enabled).toBe(false);
  });

  it("clamps transfer window minutes and formats retention badges", () => {
    expect(clampTransferWindowMinutes(0)).toBe(1);
    expect(clampTransferWindowMinutes(99)).toBe(60);
    expect(clampTransferWindowMinutes(Number.NaN)).toBe(15);

    expect(formatRetentionBadge(undefined)).toBe("Live only");
    expect(
      formatRetentionBadge({
        retention: "history",
        retentionDays: 7,
        moneySources: [],
        sensitive: false,
      }),
    ).toBe("History · 7d");
    expect(
      formatRetentionBadge({
        retention: "transactions",
        retentionDays: 30,
        moneySources: [],
        sensitive: true,
      }),
    ).toBe("Transactions only");
  });

  it("updates per-app policies and prunes default-empty entries", () => {
    let settings: NotificationStorageSettings = {
      ...DEFAULT_NOTIFICATION_STORAGE,
      largeIncomeThresholdByCurrency: {},
      perApp: {},
    };
    settings = withUpdatedAppPolicy(settings, "com.example.bank", (p) => ({
      ...p,
      retention: "history",
      retentionDays: 90,
    }));
    expect(settings.perApp["com.example.bank"]?.retentionDays).toBe(90);

    settings = withUpdatedAppPolicy(settings, "com.example.bank", (p) => ({
      ...p,
      retention: "none",
      retentionDays: 30,
    }));
    expect(settings.perApp["com.example.bank"]).toBeUndefined();

    expect(
      createMoneySourceId(
        [{ id: "src_bank_1", type: "bank", ruleTemplate: "generic_bank", enabled: true }],
        "bank",
      ),
    ).toBe("src_bank_2");
  });

  it("loads defaults when absent and normalizes stored settings", async () => {
    const empty = await loadNotificationStorageFrom(memoryStore());
    expect(empty.historyEnabled).toBe(false);

    const loaded = await loadNotificationStorageFrom(
      memoryStore({
        notificationStorage: {
          historyEnabled: true,
          transactionRetention: "forever",
        },
      }),
    );
    expect(loaded.historyEnabled).toBe(true);
    expect(loaded.transactionRetention).toBe("forever");
    expect(loaded.transferMatchWindowMinutes).toBe(15);
  });

  it("validates Android package names and migrates invalid perApp entries based on saved vault data", async () => {
    expect(isValidAndroidPackage("com.example.bank")).toBe(true);
    expect(isValidAndroidPackage("id.co.example_app.v2")).toBe(true);
    expect(isValidAndroidPackage("invalidpkg")).toBe(false);
    expect(isValidAndroidPackage("com..bank")).toBe(false);
    expect(isValidAndroidPackage("123.bank")).toBe(false);

    const store = memoryStore({
      notificationStorage: {
        version: 2,
        historyEnabled: true,
        perApp: {
          invalid_no_data: { retention: "history", retentionDays: 30, moneySources: [], sensitive: false },
          invalid_with_data: { retention: "history", retentionDays: 30, moneySources: [], sensitive: false },
          "com.example.bank": { retention: "history", retentionDays: 30, moneySources: [], sensitive: false },
        },
      },
    });

    const migrated = await loadNotificationStorageFrom(store, new Set(["invalid_with_data"]));
    expect(migrated.perApp["invalid_no_data"]).toBeUndefined();
    expect(migrated.perApp["invalid_with_data"]).toBeDefined();
    expect(migrated.perApp["com.example.bank"]).toBeDefined();

    // Persisted store was updated without the dropped entry
    const persisted = await store.get<NotificationStorageSettings>("notificationStorage");
    expect(persisted?.perApp["invalid_no_data"]).toBeUndefined();
    expect(persisted?.perApp["invalid_with_data"]).toBeDefined();

    const direct = migrateInvalidPerAppEntries(migrated, new Set(["invalid_with_data"]));
    expect(direct.dropped).toEqual([]);
    expect(direct.keptInvalid).toEqual(["invalid_with_data"]);
  });
});
