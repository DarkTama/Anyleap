import { useEffect } from "react";
import {
  Battery,
  BatteryCharging,
  Play,
  RefreshCw,
  Smartphone,
  Square,
  Stethoscope,
  Usb,
  Wifi,
} from "lucide-react";
import { DeviceStateBadge } from "@/components/DeviceStateBadge";
import { Badge } from "@/components/ui/badge";
import { getDeviceDetails } from "@/lib/tauri";
import { useAppStore } from "@/store/useAppStore";
import type { BatteryInfo, DeviceInfo, SavedDevice, SessionInfo } from "@/lib/types";
import type { StudioMode } from "./LaunchMatrix";

interface DeviceHeroCardProps {
  activeDevice: DeviceInfo | SavedDevice | null;
  selectedSerial: string | null;
  displayName: string;
  nickname?: string;
  isLive: boolean;
  isWireless: boolean;
  activeSession: SessionInfo | null;
  currentMode: StudioMode;
  battery?: BatteryInfo;
  launching: boolean;
  reconnecting: boolean;
  refreshing: boolean;
  onMirror: () => void;
  onReconnect: () => void;
  onRefresh: () => void;
  onOpenPair?: () => void;
}

const MODE_LABELS: Record<StudioMode, string> = {
  flex: "Mirror (Flex)",
  mirror: "Mirror",
  camera: "Camera",
  hifi: "Mirror (Hi-Fi)",
};

export function DeviceHeroCard({
  activeDevice,
  selectedSerial,
  displayName,
  nickname,
  isLive,
  isWireless,
  activeSession,
  currentMode,
  battery,
  launching,
  reconnecting,
  refreshing,
  onMirror,
  onReconnect,
  onRefresh,
  onOpenPair,
}: DeviceHeroCardProps) {
  const details = useAppStore((s) =>
    selectedSerial ? s.deviceDetailsCache[selectedSerial] : undefined,
  );
  const setDeviceDetails = useAppStore((s) => s.setDeviceDetails);
  const openDoctor = useAppStore((s) => s.openDoctor);

  useEffect(() => {
    if (!selectedSerial || !isLive || details) return;
    let cancelled = false;
    getDeviceDetails(selectedSerial)
      .then((res) => {
        if (!cancelled) setDeviceDetails(selectedSerial, res);
      })
      .catch(() => {
        // Best-effort; hero card still renders without getprop details.
      });
    return () => {
      cancelled = true;
    };
  }, [selectedSerial, isLive, details, setDeviceDetails]);

  if (!activeDevice) {
    return (
      <div className="flex items-center justify-between rounded-xl border border-[#222226] bg-[#141417] px-5 py-4">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-[#27272a] bg-[#18181c] text-zinc-400">
            <Smartphone className="h-5 w-5" />
          </div>
          <div>
            <h2 className="text-sm font-semibold text-zinc-100">Connect a phone</h2>
            <p className="text-xs text-zinc-400">
              Plug in via USB with debugging enabled, or pair wirelessly over Wi-Fi.
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onRefresh}
            disabled={refreshing}
            className="btn-tactile flex items-center gap-1.5 rounded-lg border border-[#27272a] bg-[#16161a] px-2.5 py-1.5 text-xs font-medium text-zinc-300 hover:border-zinc-600 hover:bg-[#1c1c21] cursor-pointer"
            title="Rescan devices"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin" : ""}`} />
          </button>
          {onOpenPair && (
            <button
              type="button"
              onClick={onOpenPair}
              className="btn-tactile flex items-center gap-1.5 rounded-lg bg-zinc-100 px-3.5 py-1.5 text-xs font-semibold text-zinc-950 hover:bg-zinc-200 cursor-pointer"
            >
              <Wifi className="h-3.5 w-3.5" />
              <span>Pair</span>
            </button>
          )}
        </div>
      </div>
    );
  }

  const deviceState = "state" in activeDevice ? activeDevice.state : "offline";
  const connectionType = details?.connection ?? (isWireless ? "wifi" : "usb");
  const resolvedTitle =
    nickname ||
    ("label" in activeDevice && activeDevice.label) ||
    details?.model ||
    displayName;

  return (
    <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-[#222226] bg-[#141417] px-4 py-3.5">
      {/* Left: Identity & Health */}
      <div className="flex items-center gap-3.5 min-w-0">
        <div
          className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border ${
            isLive
              ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-400"
              : "border-[#27272a] bg-[#18181c] text-zinc-400"
          }`}
        >
          <Smartphone className="h-5 w-5" />
        </div>

        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="truncate text-sm font-semibold text-zinc-100">
              {resolvedTitle}
            </h2>
            {details?.androidVersion && (
              <span className="font-mono text-[11px] text-zinc-400">
                Android {details.androidVersion}
              </span>
            )}
            <Badge className="gap-1 border border-[#27272a] bg-[#18181c] text-zinc-300 font-mono text-[10px]">
              {connectionType === "wifi" ? (
                <>
                  <Wifi className="h-3 w-3 text-sky-400" />
                  Wi-Fi
                </>
              ) : (
                <>
                  <Usb className="h-3 w-3 text-zinc-400" />
                  USB
                </>
              )}
            </Badge>
            <DeviceStateBadge state={deviceState} />
          </div>

          <div className="flex flex-wrap items-center gap-2.5 font-mono text-[11px] text-zinc-400">
            <span className="truncate">{selectedSerial ?? "—"}</span>
            {details?.manufacturer && (
              <>
                <span className="text-zinc-600">·</span>
                <span>{details.manufacturer}</span>
              </>
            )}
            {battery?.level !== null && battery?.level !== undefined && (
              <>
                <span className="text-zinc-600">·</span>
                <span className="inline-flex items-center gap-1 text-zinc-300">
                  {battery.charging ? (
                    <BatteryCharging className="h-3.5 w-3.5 text-emerald-400" />
                  ) : (
                    <Battery className="h-3.5 w-3.5 text-zinc-400" />
                  )}
                  {battery.level}%
                </span>
              </>
            )}
          </div>
        </div>
      </div>

      {/* Right: Primary Actions */}
      <div className="flex items-center gap-2">
        {activeSession ? (
          <button
            type="button"
            disabled={launching}
            onClick={onMirror}
            className="btn-tactile flex items-center gap-1.5 rounded-lg bg-rose-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-rose-500 cursor-pointer disabled:opacity-50"
          >
            <Square className="h-3 w-3 fill-white" />
            <span>Stop</span>
          </button>
        ) : (
          <button
            type="button"
            disabled={launching || !selectedSerial}
            onClick={onMirror}
            className="btn-tactile flex items-center gap-1.5 rounded-lg bg-zinc-100 px-3 py-1.5 text-xs font-semibold text-zinc-950 hover:bg-zinc-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Play className="h-3 w-3 fill-zinc-950" />
            <span>{MODE_LABELS[currentMode]}</span>
          </button>
        )}

        {isWireless && (
          <button
            type="button"
            disabled={reconnecting}
            onClick={onReconnect}
            className="btn-tactile flex items-center gap-1.5 rounded-lg border border-[#27272a] bg-[#16161a] px-2.5 py-1.5 text-xs font-medium text-zinc-200 hover:border-zinc-600 hover:bg-[#1c1c21] cursor-pointer disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${reconnecting ? "animate-spin" : ""}`} />
            <span>Reconnect</span>
          </button>
        )}

        <button
          type="button"
          onClick={() => openDoctor(selectedSerial)}
          className="btn-tactile flex items-center gap-1.5 rounded-lg border border-[#27272a] bg-[#16161a] px-2.5 py-1.5 text-xs font-medium text-zinc-300 hover:border-zinc-600 hover:bg-[#1c1c21] cursor-pointer"
        >
          <Stethoscope className="h-3.5 w-3.5 text-zinc-400" />
          <span>Troubleshoot</span>
        </button>

        <button
          type="button"
          onClick={onRefresh}
          disabled={refreshing}
          className="btn-tactile flex items-center justify-center rounded-lg border border-[#27272a] bg-[#16161a] p-1.5 text-zinc-400 hover:border-zinc-600 hover:text-zinc-200 cursor-pointer"
          title="Rescan devices"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin text-zinc-200" : ""}`} />
        </button>
      </div>
    </div>
  );
}
