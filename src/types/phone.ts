// Shared SPEC-27 Phone Hub types mirroring the §7 + §11 contract.

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
