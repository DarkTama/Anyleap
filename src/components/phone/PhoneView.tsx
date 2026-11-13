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
      <div className="flex flex-1 flex-col items-center justify-center bg-[#0c0c0e] p-8 text-center">
        <Smartphone className="mb-3 h-10 w-10 text-zinc-700" />
        <p className="text-sm font-medium text-zinc-300">No phone connected</p>
        <p className="mt-1 text-xs text-zinc-500">
          Connect an Android device via USB or wireless ADB to browse files, photos, and notifications.
        </p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden bg-[#0c0c0e]">
      {/* Sub-navigation & device selector */}
      <div className="flex items-center justify-between border-b border-[#222226] bg-[#101013] px-4 py-2">
        <div className="flex items-center gap-1 rounded-lg border border-[#222226] bg-[#141417] p-0.5">
          <button
            type="button"
            onClick={() => onSubTabChange("files")}
            className={`flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
              subTab === "files"
                ? "bg-[#222228] text-zinc-100 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <FolderOpen className="h-3.5 w-3.5" />
            Files
          </button>
          <button
            type="button"
            onClick={() => onSubTabChange("photos")}
            className={`flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
              subTab === "photos"
                ? "bg-[#222228] text-zinc-100 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <ImageIcon className="h-3.5 w-3.5" />
            Photos
          </button>
          <button
            type="button"
            onClick={() => onSubTabChange("notifications")}
            className={`flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
              subTab === "notifications"
                ? "bg-[#222228] text-zinc-100 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <Bell className="h-3.5 w-3.5" />
            Notifications
          </button>
        </div>

        <div className="flex items-center gap-2">
          <span className="text-[11px] text-zinc-500">Device</span>
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
