export type RetentionPolicy = "none" | "history" | "transactions";
export type RetentionDays = 7 | 30 | 90;
export type MoneySourceType = "bank" | "ewallet" | "paylater" | "credit_card";
export type RuleTemplateId =
  | "generic_bank"
  | "generic_ewallet"
  | "generic_paylater"
  | "generic_credit_card"
  | "cobranded_card_in_shop_app"
  | "custom";
export type TransactionRetention = "1y" | "2y" | "forever";

export interface MoneySourceConfig {
  id: string;
  type: MoneySourceType;
  ruleTemplate: RuleTemplateId | string;
  enabled: boolean;
  label?: string;
}

export interface AppNotificationPolicy {
  retention: RetentionPolicy;
  retentionDays: RetentionDays;
  moneySources: MoneySourceConfig[];
  sensitive: boolean;
}

export interface NotificationStorageSettings {
  version: 2;
  historyEnabled: boolean;
  backgroundCapture: boolean;
  transactionRetention: TransactionRetention;
  transferMatchWindowMinutes: number;
  largeIncomeThresholdByCurrency: Record<string, number>;
  perApp: Record<string, AppNotificationPolicy>;
}

export const MONEY_SOURCE_TYPE_LABELS: Record<MoneySourceType, string> = {
  bank: "Bank",
  ewallet: "E-wallet",
  paylater: "Paylater",
  credit_card: "Credit card",
};

export const RULE_TEMPLATE_LABELS: Record<string, string> = {
  generic_bank: "Generic bank",
  generic_ewallet: "Generic e-wallet",
  generic_paylater: "Generic paylater",
  generic_credit_card: "Generic credit card",
  cobranded_card_in_shop_app: "Co-branded card in shopping app",
  custom: "Custom",
};

export function formatRuleTemplateLabel(ruleTemplate: string): string {
  return RULE_TEMPLATE_LABELS[ruleTemplate] ?? ruleTemplate;
}

export const RULE_TEMPLATES_BY_TYPE: Record<
  MoneySourceType,
  Array<{ id: RuleTemplateId; label: string }>
> = {
  bank: [{ id: "generic_bank", label: "Generic bank" }],
  ewallet: [{ id: "generic_ewallet", label: "Generic e-wallet" }],
  paylater: [{ id: "generic_paylater", label: "Generic paylater" }],
  credit_card: [{ id: "generic_credit_card", label: "Generic credit card" }],
};

export const DEFAULT_APP_POLICY: AppNotificationPolicy = {
  retention: "none",
  retentionDays: 30,
  moneySources: [],
  sensitive: false,
};

export const DEFAULT_NOTIFICATION_STORAGE: NotificationStorageSettings = {
  version: 2,
  historyEnabled: false,
  backgroundCapture: false,
  transactionRetention: "1y",
  transferMatchWindowMinutes: 15,
  largeIncomeThresholdByCurrency: {},
  perApp: {},
};

const VALID_RETENTION: ReadonlySet<string> = new Set(["none", "history", "transactions"]);
const VALID_DAYS: ReadonlySet<number> = new Set([7, 30, 90]);
const VALID_SOURCE_TYPES: ReadonlySet<string> = new Set([
  "bank",
  "ewallet",
  "paylater",
  "credit_card",
]);
const VALID_TX_RETENTION: ReadonlySet<string> = new Set(["1y", "2y", "forever"]);

export function clampTransferWindowMinutes(value: number): number {
  if (!Number.isFinite(value)) return 15;
  return Math.max(1, Math.min(60, Math.round(value)));
}

export function normalizeMoneySource(raw: unknown, fallbackIdx = 1): MoneySourceConfig | null {
  if (!raw || typeof raw !== "object") return null;
  const obj = raw as Record<string, unknown>;
  const type = typeof obj.type === "string" && VALID_SOURCE_TYPES.has(obj.type)
    ? (obj.type as MoneySourceType)
    : null;
  if (!type) return null;

  const defaultTemplate = RULE_TEMPLATES_BY_TYPE[type][0].id;
  const ruleTemplate =
    typeof obj.ruleTemplate === "string" && obj.ruleTemplate.trim()
      ? obj.ruleTemplate.trim()
      : defaultTemplate;
  const id =
    typeof obj.id === "string" && obj.id.trim()
      ? obj.id.trim()
      : `src_${type}_${fallbackIdx}`;
  const enabled = typeof obj.enabled === "boolean" ? obj.enabled : true;
  const label =
    typeof obj.label === "string" && obj.label.trim() ? obj.label.trim() : undefined;

  return { id, type, ruleTemplate, enabled, ...(label ? { label } : {}) };
}

export function normalizeAppPolicy(raw: unknown): AppNotificationPolicy {
  if (!raw || typeof raw !== "object") return { ...DEFAULT_APP_POLICY, moneySources: [] };
  const obj = raw as Record<string, unknown>;
  const retention =
    typeof obj.retention === "string" && VALID_RETENTION.has(obj.retention)
      ? (obj.retention as RetentionPolicy)
      : "none";
  const retentionDays =
    typeof obj.retentionDays === "number" && VALID_DAYS.has(obj.retentionDays)
      ? (obj.retentionDays as RetentionDays)
      : 30;
  const rawSources = Array.isArray(obj.moneySources) ? obj.moneySources : [];
  const moneySources: MoneySourceConfig[] = [];
  const seenIds = new Set<string>();
  rawSources.forEach((s, idx) => {
    const norm = normalizeMoneySource(s, idx + 1);
    if (norm && !seenIds.has(norm.id)) {
      seenIds.add(norm.id);
      moneySources.push(norm);
    }
  });
  const sensitive = typeof obj.sensitive === "boolean" ? obj.sensitive : false;
  return { retention, retentionDays, moneySources, sensitive };
}

export function normalizeNotificationStorage(raw: unknown): NotificationStorageSettings {
  if (!raw || typeof raw !== "object") {
    return {
      ...DEFAULT_NOTIFICATION_STORAGE,
      largeIncomeThresholdByCurrency: {},
      perApp: {},
    };
  }
  const obj = raw as Record<string, unknown>;
  const historyEnabled =
    typeof obj.historyEnabled === "boolean" ? obj.historyEnabled : false;
  const backgroundCapture =
    typeof obj.backgroundCapture === "boolean" ? obj.backgroundCapture : false;
  const transactionRetention =
    typeof obj.transactionRetention === "string" &&
    VALID_TX_RETENTION.has(obj.transactionRetention)
      ? (obj.transactionRetention as TransactionRetention)
      : "1y";
  const transferMatchWindowMinutes =
    typeof obj.transferMatchWindowMinutes === "number"
      ? clampTransferWindowMinutes(obj.transferMatchWindowMinutes)
      : 15;

  const largeIncomeThresholdByCurrency: Record<string, number> = {};
  if (
    obj.largeIncomeThresholdByCurrency &&
    typeof obj.largeIncomeThresholdByCurrency === "object"
  ) {
    for (const [k, v] of Object.entries(
      obj.largeIncomeThresholdByCurrency as Record<string, unknown>,
    )) {
      const code = k.trim().toUpperCase();
      if (code && typeof v === "number" && Number.isFinite(v) && v > 0) {
        largeIncomeThresholdByCurrency[code] = Math.round(v);
      }
    }
  }

  const perApp: Record<string, AppNotificationPolicy> = {};
  if (obj.perApp && typeof obj.perApp === "object") {
    for (const [pkg, pol] of Object.entries(obj.perApp as Record<string, unknown>)) {
      const trimmed = pkg.trim();
      if (!trimmed) continue;
      perApp[trimmed] = normalizeAppPolicy(pol);
    }
  }

  return {
    version: 2,
    historyEnabled,
    backgroundCapture,
    transactionRetention,
    transferMatchWindowMinutes,
    largeIncomeThresholdByCurrency,
    perApp,
  };
}

export function getAppPolicy(
  settings: NotificationStorageSettings,
  pkg: string,
): AppNotificationPolicy {
  return settings.perApp[pkg] ?? DEFAULT_APP_POLICY;
}

export function formatRetentionBadge(policy?: AppNotificationPolicy): string {
  if (!policy || policy.retention === "none") return "Live only";
  if (policy.retention === "history") return `History · ${policy.retentionDays}d`;
  return "Transactions only";
}

export function formatMoneySourceLabel(source: MoneySourceConfig): string {
  return source.label || MONEY_SOURCE_TYPE_LABELS[source.type];
}

export function createMoneySourceId(
  existing: MoneySourceConfig[],
  type: MoneySourceType,
): string {
  const used = new Set(existing.map((s) => s.id));
  let idx = 1;
  while (used.has(`src_${type}_${idx}`)) {
    idx += 1;
  }
  return `src_${type}_${idx}`;
}

export function isPolicyDefaultEmpty(policy: AppNotificationPolicy): boolean {
  return (
    policy.retention === "none" &&
    policy.retentionDays === 30 &&
    policy.moneySources.length === 0 &&
    !policy.sensitive
  );
}

export const ANDROID_PACKAGE_REGEX = /^[a-zA-Z][a-zA-Z0-9_]*(\.[a-zA-Z][a-zA-Z0-9_]*)+$/;

export function isValidAndroidPackage(pkg: string): boolean {
  return ANDROID_PACKAGE_REGEX.test(pkg.trim());
}

export function migrateInvalidPerAppEntries(
  settings: NotificationStorageSettings,
  savedPackages: ReadonlySet<string>,
): { settings: NotificationStorageSettings; dropped: string[]; keptInvalid: string[] } {
  const dropped: string[] = [];
  const keptInvalid: string[] = [];
  const nextPerApp: Record<string, AppNotificationPolicy> = {};

  for (const [pkg, policy] of Object.entries(settings.perApp)) {
    if (isValidAndroidPackage(pkg)) {
      nextPerApp[pkg] = policy;
      continue;
    }
    if (savedPackages.has(pkg)) {
      nextPerApp[pkg] = policy;
      keptInvalid.push(pkg);
    } else {
      dropped.push(pkg);
    }
  }

  if (dropped.length === 0) {
    return { settings, dropped, keptInvalid };
  }
  return {
    settings: {
      ...settings,
      perApp: nextPerApp,
    },
    dropped,
    keptInvalid,
  };
}

export function withUpdatedAppPolicy(
  settings: NotificationStorageSettings,
  pkg: string,
  updater: (prev: AppNotificationPolicy) => AppNotificationPolicy,
): NotificationStorageSettings {
  const prev = getAppPolicy(settings, pkg);
  const nextPolicy = normalizeAppPolicy(updater(prev));
  const nextPerApp = { ...settings.perApp };
  if (isPolicyDefaultEmpty(nextPolicy)) {
    delete nextPerApp[pkg];
  } else {
    nextPerApp[pkg] = nextPolicy;
  }
  return {
    ...settings,
    perApp: nextPerApp,
  };
}
