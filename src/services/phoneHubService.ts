import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { OpenAppResult } from "@/lib/types";
import type { NotificationStorageSettings } from "@/lib/notificationStorage";
import { resolveSelectedLaunchSettings } from "@/lib/modeActions";
import type {
  Album,
  Conversation,
  CorrectionResult,
  DeleteResult,
  LiabilityResolveResult,
  MediaKind,
  MediaPage,
  MoneyVaultSnapshot,
  PhoneApp,
  PhoneAudioStateEvent,
  PhoneHubSection,
  PhoneMessagesUpdatedEvent,
  PhoneNotificationsEvent,
  PhoneOverview,
  PhoneOverviewEvent,
  StoredNotification,
  TxClassification,
  VaultStatus,
} from "@/types/phone";

export const phoneHubWatch = (serial: string, sections: PhoneHubSection[]) =>
  invoke<void>("phone_hub_watch", { serial, sections });

export const phoneHubUnwatch = (serial: string) =>
  invoke<void>("phone_hub_unwatch", { serial });

export const phoneGetOverview = (serial: string, refresh = false) =>
  invoke<PhoneOverview>("phone_get_overview", { serial, refresh });

export const phoneHubSetSettings = (notificationPollSecs: number) =>
  invoke<void>("phone_hub_set_settings", { notificationPollSecs });

export const phoneListApps = (serial: string, refresh = false) =>
  invoke<PhoneApp[]>("phone_list_apps", { serial, refresh });

export const phoneGetPinnedApps = (serial: string) =>
  invoke<string[]>("phone_get_pinned_apps", { serial });

export const phoneSetAppPinned = (serial: string, packageName: string, pinned: boolean) =>
  invoke<string[]>("phone_set_app_pinned", { serial, package: packageName, pinned });

/** Opens in the running Flex/Mirror session, else a new window built from the Devices preset. */
export const phoneOpenApp = (serial: string, packageName: string) =>
  invoke<OpenAppResult>("phone_open_app", {
    serial,
    package: packageName,
    settings: resolveSelectedLaunchSettings(),
  });

export const phoneGetConversations = (serial: string) =>
  invoke<Conversation[]>("phone_get_conversations", { serial });

export const phoneMarkConversationRead = (serial: string, key: string) =>
  invoke<void>("phone_mark_conversation_read", { serial, key });

export const phoneListAlbums = (serial: string) =>
  invoke<Album[]>("phone_list_albums", { serial });

export const phoneListMedia = (
  serial: string,
  kind: MediaKind,
  album: string | null,
  offset: number,
  limit: number,
) => invoke<MediaPage>("phone_list_media", { serial, kind, album, offset, limit });

export const phoneMediaThumbnail = (serial: string, id: string) =>
  invoke<string | null>("phone_media_thumbnail", { serial, id });

export const phoneOpenMedia = (serial: string, id: string) =>
  invoke<void>("phone_open_media", { serial, id });

export const phoneAudioStart = (serial: string) =>
  invoke<void>("phone_audio_start", { serial });

export const phoneAudioStop = (serial: string) =>
  invoke<void>("phone_audio_stop", { serial });

export const onPhoneOverview = (cb: (payload: PhoneOverviewEvent) => void): Promise<UnlistenFn> =>
  listen<PhoneOverviewEvent>("phone-overview", (e) => cb(e.payload));

export const onPhoneNotifications = (
  cb: (payload: PhoneNotificationsEvent) => void,
): Promise<UnlistenFn> =>
  listen<PhoneNotificationsEvent>("phone-notifications", (e) => cb(e.payload));

export const onPhoneMessagesUpdated = (
  cb: (payload: PhoneMessagesUpdatedEvent) => void,
): Promise<UnlistenFn> =>
  listen<PhoneMessagesUpdatedEvent>("phone-messages-updated", (e) => cb(e.payload));

export const onPhoneAudioState = (
  cb: (payload: PhoneAudioStateEvent) => void,
): Promise<UnlistenFn> =>
  listen<PhoneAudioStateEvent>("phone-audio-state", (e) => cb(e.payload));

export const phoneSetNotificationStorageConfig = (
  config: NotificationStorageSettings,
) => invoke<VaultStatus>("phone_set_notification_storage_config", { config });

export const phoneGetVaultStatus = () =>
  invoke<VaultStatus>("phone_get_vault_status");

export const phoneGetSavedNotificationDevices = () =>
  invoke<string[]>("phone_get_saved_notification_devices");

export const phoneGetSavedNotificationPackages = () =>
  invoke<string[]>("phone_get_saved_notification_packages");

export const phoneGetNotificationHistory = (
  hwSerial: string,
  limit = 200,
  beforeUnix?: number,
) =>
  invoke<StoredNotification[]>("phone_get_notification_history", {
    hwSerial,
    limit,
    beforeUnix: beforeUnix ?? null,
  });

export const phoneSetNotificationHidden = (dedupeId: string, hidden: boolean) =>
  invoke<void>("phone_set_notification_hidden", { dedupeId, hidden });

export const phoneDeleteAppNotificationData = (
  packageName: string,
  hwSerial?: string,
  deleteRules = false,
) =>
  invoke<DeleteResult>("phone_delete_app_notification_data", {
    package: packageName,
    hwSerial: hwSerial ?? null,
    deleteRules,
  });

export const phoneDeleteAllNotificationData = () =>
  invoke<DeleteResult>("phone_delete_all_notification_data");

export const phoneGetTransactions = (params?: {
  hwSerial?: string | null;
  currency?: string | null;
  fromMs?: number | null;
  toMs?: number | null;
}) =>
  invoke<MoneyVaultSnapshot>("phone_get_transactions", {
    hwSerial: params?.hwSerial ?? null,
    currency: params?.currency ?? null,
    fromMs: params?.fromMs ?? null,
    toMs: params?.toMs ?? null,
  });

export const phoneResolveLiabilityEvent = (
  eventId: string,
  linkedTxId: string | null,
  ignore: boolean,
) =>
  invoke<LiabilityResolveResult>("phone_resolve_liability_event", {
    eventId,
    linkedTxId,
    ignore,
  });

export const phoneApplyTransactionCorrection = (
  txId: string,
  classification: TxClassification | null,
  category: string | null = null,
  applyToSimilar = false,
) =>
  invoke<CorrectionResult>("phone_apply_transaction_correction", {
    txId,
    classification,
    category,
    applyToSimilar,
  });
