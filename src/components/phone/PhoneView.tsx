import { Bell, FolderOpen, Image as ImageIcon, Smartphone } from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import type { PhoneNotification } from "@/lib/types";
import { selectClass, optionClass } from "@/lib/styles";
import { FilesView } from "./FilesView";
import { PhotosView } from "./PhotosView";
import { NotificationsView } from "./NotificationsView";

export type PhoneSubTab = "files" | "photos" | "notifications";

interface PhoneViewProps {
  subTab: PhoneSubTab;
  onSubTabChange: (tab: PhoneSubTab) => void;
  selectedSerial: string | null;
  onSelectSerial: (serial: string) => void;
  notifications: PhoneNotification[];
  notificationsLoading: boolean;
  notificationsError: string | null;
  onRefreshNotifications: () => void;
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

  const activeSerial =
    selectedSerial && onlineDevices.some((d) => d.serial === selectedSerial)
      ? selectedSerial
      : onlineDevices[0]?.serial ?? null;

  if (!activeSerial) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center bg-ink-950 p-8 text-center">
        <Smartphone className="mb-3 h-10 w-10 text-ink-700" />
        <p className="text-sm font-medium text-text-strong">No phone connected</p>
        <p className="mt-1 text-xs text-text-muted">
          Connect an Android device via USB or wireless ADB to browse files, photos, and notifications.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-ink-950">
      {/* Sub-navigation & device selector */}
      <div className="flex items-center justify-between border-b border-ink-700 bg-ink-900 px-4 py-2">
        <div className="flex items-center gap-1 rounded-lg border border-ink-700 bg-ink-950 p-0.5">
          <button
            type="button"
            onClick={() => onSubTabChange("files")}
            className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
              subTab === "files"
                ? "bg-ink-800 text-gold-400"
                : "text-text-muted hover:text-text-strong"
            }`}
          >
            <FolderOpen className="h-3.5 w-3.5" />
            Files
          </button>
          <button
            type="button"
            onClick={() => onSubTabChange("photos")}
            className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
              subTab === "photos"
                ? "bg-ink-800 text-gold-400"
                : "text-text-muted hover:text-text-strong"
            }`}
          >
            <ImageIcon className="h-3.5 w-3.5" />
            Photos
          </button>
          <button
            type="button"
            onClick={() => onSubTabChange("notifications")}
            className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
              subTab === "notifications"
                ? "bg-ink-800 text-gold-400"
                : "text-text-muted hover:text-text-strong"
            }`}
          >
            <Bell className="h-3.5 w-3.5" />
            Notifications
          </button>
        </div>

        <div className="flex items-center gap-2">
          <span className="text-[11px] text-text-muted">Device</span>
          <select
            className={`${selectClass} !w-auto min-w-[180px] !py-1 text-xs`}
            value={activeSerial}
            onChange={(e) => onSelectSerial(e.target.value)}
          >
            {onlineDevices.map((d) => (
              <option key={d.serial} value={d.serial} className={optionClass}>
                {nicknames[d.serial] || d.model || d.serial}
              </option>
            ))}
          </select>
        </div>
      </div>

      {subTab === "files" && <FilesView serial={activeSerial} />}
      {subTab === "photos" && <PhotosView serial={activeSerial} />}
      {subTab === "notifications" && (
        <NotificationsView
          notifications={notifications}
          loading={notificationsLoading}
          error={notificationsError}
          onRefresh={onRefreshNotifications}
        />
      )}
    </div>
  );
}
