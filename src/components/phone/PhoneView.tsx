import { reportDeviceError } from "@/lib/deviceHealth";
import { useEffect, useState } from "react";
import {
  AppWindow,
  Bell,
  FolderOpen,
  Images,
  LayoutDashboard,
  MessageSquare,
  Smartphone,
  Usb,
  Wifi,
} from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import { isWifiSerial } from "@/lib/deviceStatus";
import { selectClass, optionClass } from "@/lib/styles";
import {
  onPhoneAudioState,
  onPhoneMessagesUpdated,
  onPhoneOverview,
  phoneGetConversations,
  phoneGetOverview,
  phoneHubUnwatch,
  phoneHubWatch,
} from "@/services/phoneHubService";
import {
  getTotalUnreadMessages,
  usePhoneHubStore,
} from "@/stores/phoneHubStore";
import type { PhoneHubNotification } from "@/types/phone";
import { OverviewView } from "./OverviewView";
import { MessagesView } from "./MessagesView";
import { NotificationsView } from "./NotificationsView";
import { GalleryView } from "./GalleryView";
import { FilesView } from "./FilesView";
import { AppsView } from "./AppsView";

export type PhoneSubTab =
  | "overview"
  | "messages"
  | "notifications"
  | "gallery"
  | "files"
  | "apps";

interface PhoneViewProps {
  subTab: PhoneSubTab;
  onSubTabChange: (tab: PhoneSubTab) => void;
  selectedSerial: string | null;
  onSelectSerial: (serial: string) => void;
  notifications: PhoneHubNotification[];
  notificationsLoading: boolean;
  notificationsError: string | null;
  onRefreshNotifications: () => void;
}

const NAV_ITEMS: Array<{
  id: PhoneSubTab;
  label: string;
  Icon: typeof LayoutDashboard;
}> = [
  { id: "overview", label: "Overview", Icon: LayoutDashboard },
  { id: "messages", label: "Messages", Icon: MessageSquare },
  { id: "notifications", label: "Notifications", Icon: Bell },
  { id: "gallery", label: "Gallery", Icon: Images },
  { id: "files", label: "Files", Icon: FolderOpen },
  { id: "apps", label: "Apps", Icon: AppWindow },
];

const SECTION_TITLES: Record<PhoneSubTab, string> = {
  overview: "Overview",
  messages: "Messages",
  notifications: "Notifications",
  gallery: "Gallery",
  files: "Files",
  apps: "Apps",
};

function formatUpdatedAgo(updatedAtMs: number | undefined, nowMs: number): string {
  if (!updatedAtMs) return "Waiting for phone data…";
  const secs = Math.max(0, Math.floor((nowMs - updatedAtMs) / 1000));
  if (secs < 2) return "Updated just now";
  if (secs < 60) return `Updated ${secs} s ago`;
  const mins = Math.floor(secs / 60);
  return `Updated ${mins} m ago`;
}

export function PhoneView({
  subTab,
  onSubTabChange,
  selectedSerial,
  onSelectSerial,
  notifications,
  notificationsLoading,
  notificationsError,
  onRefreshNotifications,
}: PhoneViewProps) {
  const devices = useAppStore((s) => s.devices);
  const nicknames = useAppStore((s) => s.nicknames);
  const onlineDevices = devices.filter((d) => d.state === "device");

  const activeDevice =
    (selectedSerial && onlineDevices.find((d) => d.serial === selectedSerial)) ||
    onlineDevices[0] ||
    null;
  const activeSerial = activeDevice?.serial ?? null;

  const isWireless = activeDevice
    ? Boolean(activeDevice.transports?.wifi && !activeDevice.transports?.usb) ||
      isWifiSerial(activeDevice.serial)
    : false;

  const overview = usePhoneHubStore((s) =>
    activeSerial ? s.overviewsBySerial[activeSerial] : undefined,
  );
  const isStale = usePhoneHubStore((s) =>
    activeSerial ? Boolean(s.staleBySerial[activeSerial]) : false,
  );
  const conversations = usePhoneHubStore((s) =>
    activeSerial ? s.conversationsBySerial[activeSerial] : undefined,
  );
  const mutedPackages = usePhoneHubStore((s) => s.mutedPackages);
  const applyOverview = usePhoneHubStore((s) => s.applyOverview);
  const applyAudioState = usePhoneHubStore((s) => s.applyAudioState);
  const applyConversations = usePhoneHubStore((s) => s.applyConversations);

  const unreadMessages = getTotalUnreadMessages(conversations);

  const [galleryAlbumLabel, setGalleryAlbumLabel] = useState("All albums");
  const [nowMs, setNowMs] = useState(() => Date.now());

  // Tick every 5s to keep "Updated X s ago" fresh on Overview
  useEffect(() => {
    if (subTab !== "overview") return;
    setNowMs(Date.now());
    const id = setInterval(() => setNowMs(Date.now()), 5000);
    return () => clearInterval(id);
  }, [subTab]);

  // Watch/unwatch backend phone hub + hydrate initial overview & conversations
  useEffect(() => {
    if (!activeSerial) return;
    let cancelled = false;

    void phoneHubWatch(activeSerial, ["overview", "notifications"]).catch(() => {});
    phoneGetOverview(activeSerial, true)
      .catch(async (firstErr) => {
        // Dropped wireless transport: reconnect via mDNS, then retry once.
        if (!(await reportDeviceError(firstErr, activeSerial))) throw firstErr;
        return phoneGetOverview(activeSerial, true);
      })
      .then((ov) => {
        if (!cancelled) applyOverview(activeSerial, ov);
      })
      .catch(() => {});
    phoneGetConversations(activeSerial)
      .then((convs) => {
        if (!cancelled) applyConversations(activeSerial, convs);
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      void phoneHubUnwatch(activeSerial).catch(() => {});
    };
  }, [activeSerial, applyOverview, applyConversations]);

  // Subscribe to phone hub events while PhoneView is mounted
  useEffect(() => {
    const unlisteners = [
      onPhoneOverview((payload) => {
        applyOverview(payload.serial, payload.overview);
      }),
      onPhoneAudioState((payload) => {
        applyAudioState(payload.serial, payload.playing);
      }),
      onPhoneMessagesUpdated((payload) => {
        phoneGetConversations(payload.serial)
          .then((convs) => applyConversations(payload.serial, convs))
          .catch(() => {});
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => {}));
    };
  }, [applyOverview, applyAudioState, applyConversations]);

  if (!activeSerial || !activeDevice) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center bg-ink-950 p-8 text-center">
        <Smartphone className="mb-3 h-10 w-10 text-ink-700" />
        <p className="text-sm font-medium text-text-strong">No phone connected</p>
        <p className="mt-1 text-xs text-text-muted">
          Connect an Android device via USB or wireless ADB to view overview, messages, notifications, gallery, files, and apps.
        </p>
      </div>
    );
  }

  let subtitle = "";
  if (subTab === "overview") {
    subtitle = isStale
      ? "Stale — phone disconnected"
      : formatUpdatedAgo(overview?.updatedAtMs, nowMs);
  } else if (subTab === "messages") {
    subtitle = "Built from chat notifications · memory only";
  } else if (subTab === "notifications") {
    const notifCount = notifications.length;
    const mutedCount = mutedPackages.length;
    subtitle = `${notifCount} ${notifCount === 1 ? "notification" : "notifications"} · ${mutedCount} ${mutedCount === 1 ? "app" : "apps"} muted`;
  } else if (subTab === "gallery") {
    subtitle = galleryAlbumLabel;
  } else if (subTab === "files") {
    subtitle = "Internal storage";
  } else if (subTab === "apps") {
    subtitle = "Cached per device";
  }

  return (
    <div className="flex flex-1 overflow-hidden bg-ink-950">
      {/* Labelled vertical sidebar */}
      <nav
        aria-label="Phone sections"
        className="flex w-[208px] shrink-0 flex-col gap-0.5 border-r border-ink-700 bg-ink-900 px-2 py-3"
      >
        <div className="px-2.5 pt-1 pb-2 font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
          Phone
        </div>
        {NAV_ITEMS.map(({ id, label, Icon }) => {
          const isCurrent = subTab === id;
          return (
            <button
              key={id}
              type="button"
              aria-current={isCurrent ? "page" : undefined}
              onClick={() => onSubTabChange(id)}
              className={`btn-tactile flex items-center gap-2.5 rounded-lg border px-2.5 py-2 text-left text-xs font-medium cursor-pointer ${
                isCurrent
                  ? "border-ink-700 bg-ink-800 text-gold-400"
                  : "border-transparent text-text hover:bg-ink-800 hover:text-text-strong"
              }`}
            >
              <Icon className="h-3.5 w-3.5 shrink-0" />
              <span className="min-w-0 flex-1 truncate">{label}</span>
              {id === "messages" && unreadMessages > 0 && (
                <span
                  aria-label={`${unreadMessages} unread`}
                  className="inline-flex min-w-[18px] items-center justify-center rounded-full bg-gold-400 px-1.5 py-0.5 font-mono text-[10px] font-bold leading-none text-ink-950"
                >
                  {unreadMessages}
                </span>
              )}
            </button>
          );
        })}
      </nav>

      {/* Main content area */}
      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <header className="flex min-h-[52px] shrink-0 items-center justify-between gap-3 border-b border-ink-700 bg-ink-900 px-4 py-2">
          <div className="min-w-0">
            <h1 className="text-sm font-semibold leading-5 text-text-strong">
              {SECTION_TITLES[subTab]}
            </h1>
            <div className="truncate text-[11px] text-text-muted">{subtitle}</div>
          </div>

          <div className="flex shrink-0 items-center gap-2">
            <label htmlFor="phone-device-select" className="text-[11px] text-text-muted">
              Device
            </label>
            <select
              id="phone-device-select"
              className={`${selectClass} !w-auto min-w-[160px] !py-1 text-xs`}
              value={activeSerial}
              onChange={(e) => onSelectSerial(e.target.value)}
            >
              {onlineDevices.map((d) => (
                <option key={d.serial} value={d.serial} className={optionClass}>
                  {nicknames[d.serial] || d.model || d.serial}
                </option>
              ))}
            </select>
            <span className="inline-flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 font-mono text-[11px] leading-[14px] text-text">
              {isWireless ? (
                <>
                  <Wifi className="h-3.5 w-3.5 text-mist-300" />
                  Wireless
                </>
              ) : (
                <>
                  <Usb className="h-3.5 w-3.5 text-mist-300" />
                  USB
                </>
              )}
            </span>
          </div>
        </header>

        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          {subTab === "overview" && (
            <OverviewView serial={activeSerial} isWireless={isWireless} />
          )}
          {subTab === "messages" && <MessagesView serial={activeSerial} />}
          {subTab === "notifications" && (
            <NotificationsView
              serial={activeSerial}
              notifications={notifications}
              loading={notificationsLoading}
              error={notificationsError}
              onRefresh={onRefreshNotifications}
            />
          )}
          {subTab === "gallery" && (
            <GalleryView
              serial={activeSerial}
              onAlbumChange={setGalleryAlbumLabel}
            />
          )}
          {subTab === "files" && <FilesView serial={activeSerial} />}
          {subTab === "apps" && <AppsView serial={activeSerial} />}
        </div>
      </div>
    </div>
  );
}
