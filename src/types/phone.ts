// Shared SPEC-27 Phone Hub types mirroring the §7 + §11 contract.
import type { MoneySourceType } from "@/lib/notificationStorage";

export interface PhoneBattery {
  level: number;
  charging: boolean;
  tempC: number | null;
  health: string | null;
}

export interface PhoneBytesUsage {
  usedBytes: number;
  totalBytes: number;
}

export interface PhoneWifiInfo {
  ssid: string | null;
  signalLevel: number | null; // 0-4 | null
}

export interface PhoneCellularInfo {
  operator: string | null;
  signalLevel: number | null; // 0-4 | null
}

export type PhoneScreenState = "on" | "off" | "locked";

export interface PhoneOverview {
  model: string | null;
  brand: string | null;
  androidVersion: string | null;
  securityPatch: string | null;
  battery: PhoneBattery | null;
  storage: PhoneBytesUsage | null;
  ram: PhoneBytesUsage | null;
  wifi: PhoneWifiInfo | null;
  cellular: PhoneCellularInfo | null;
  screen: PhoneScreenState | null;
  uptimeSecs: number | null;
  updatedAtMs: number;
}

export interface PhoneOverviewEvent {
  serial: string;
  overview: PhoneOverview;
}

export interface PhoneApp {
  package: string;
  label: string;
  system: boolean;
}

export interface PhoneHubNotification {
  key: string;
  package: string;
  appLabel: string;
  title: string;
  text: string;
  postedUnix: number;
}

export interface PhoneNotificationsEvent {
  serial: string;
  items: PhoneHubNotification[];
}

export interface ChatMessage {
  sender: string;
  text: string;
  timestampMs: number;
}

export interface Conversation {
  package: string;
  appLabel: string;
  key: string;
  title: string;
  isGroup: boolean;
  messages: ChatMessage[];
  unread: number;
}

export interface PhoneMessagesUpdatedEvent {
  serial: string;
  conversationKey: string;
}

export interface Album {
  name: string;
  photoCount: number;
  videoCount: number;
}

export type MediaKind = "all" | "photo" | "video";
export type MediaItemKind = "photo" | "video";

export interface MediaItem {
  id: string;
  kind: MediaItemKind;
  name: string;
  album: string;
  path: string;
  dateTakenMs: number;
  sizeBytes: number;
  width: number | null;
  height: number | null;
  durationMs: number | null;
}

export interface MediaPage {
  items: MediaItem[];
  total: number;
}

export interface PhoneAudioStateEvent {
  serial: string;
  playing: boolean;
}

export type PhoneHubSection = "overview" | "notifications";

export interface StoredNotification {
  dedupeId: string;
  hwSerial: string;
  key: string;
  package: string;
  appLabel: string;
  title: string;
  text: string;
  postedUnix: number;
  capturedUnix: number;
  hiddenLocal: boolean;
}

export interface DeleteResult {
  deletedNotifications: number;
  deletedTransactions: number;
  deletedLiabilityEvents: number;
}

export interface VaultStatus {
  historyEnabled: boolean;
  vaultError: string | null;
  savedHwSerials: string[];
}

export type TxDirection = "in" | "out";

export type TxClassification =
  | "normal"
  | "transfer"
  | "possible_transfer"
  | "debt_payment"
  | "repayment_received"
  | "ignored";

export type TransferHint =
  | "virtual_account"
  | "top_up"
  | "payment"
  | "bill"
  | "repayment";

export type LiabilityEventType = "bill_paid";

export type LiabilityEventStatus =
  | "linked"
  | "unmatched_needs_review"
  | "ignored";

export interface StoredTransaction {
  id: string;
  dedupeId: string;
  hwSerial: string;
  package: string;
  sourceId: string;
  appLabel: string;
  sourceType: MoneySourceType;
  subAccount: string | null;
  direction: TxDirection;
  amountMinor: number;
  scale: number;
  currency: string;
  category: string;
  classification: TxClassification;
  manualClassification: TxClassification | null;
  manualCategory: string | null;
  transferPairId: string | null;
  linkedLiabilityEventId: string | null;
  linkedLiabilitySourceId: string | null;
  linkedSubAccount: string | null;
  transferHint: TransferHint | null;
  similarityHash: string | null;
  statementDueMinor: number | null;
  statementDueUnix: number | null;
  timestampMs: number;
  ruleId: string;
}

export interface LiabilityEvent {
  id: string;
  dedupeId: string;
  hwSerial: string;
  package: string;
  sourceId: string;
  appLabel: string;
  sourceType: MoneySourceType;
  eventType: LiabilityEventType;
  subAccount: string | null;
  status: LiabilityEventStatus;
  linkedTxId: string | null;
  resolvedAmountMinor: number | null;
  resolvedCurrency: string | null;
  scale: number | null;
  manualLocked: boolean;
  duplicateOf?: string | null;
  timestampMs: number;
  ruleId: string;
}

export interface MoneyVaultSnapshot {
  transactions: StoredTransaction[];
  liabilityEvents: LiabilityEvent[];
  subAccountNicknames: Record<string, string>;
  currencies: string[];
  ruleWarnings: string[];
}

export interface LiabilityResolveResult {
  event: LiabilityEvent;
}

export interface CorrectionResult {
  updatedCount: number;
  transaction: StoredTransaction;
}
