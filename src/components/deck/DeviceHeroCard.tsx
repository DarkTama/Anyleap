import { useEffect } from "react";
import {
  ArrowRight,
  Play,
  RefreshCw,
  Smartphone,
  Square,
  Stethoscope,
  Wifi,
} from "lucide-react";
import { MSG_NEEDS_REPAIR, type DeviceStatus } from "@/lib/deviceStatus";
import { getDeviceDetails } from "@/lib/tauri";
import { useAppStore } from "@/store/useAppStore";
import type { BatteryInfo, DeviceInfo, SavedDevice, SessionInfo } from "@/lib/types";
import type { StudioMode } from "./LaunchMatrix";
import type { PhoneOverview } from "@/types/phone";
import { resolveBatteryChipData } from "./phoneOverviewStore";
import {
  BatteryChip,
  StatusChip,
  TransportChip,
  type TransportKind,
} from "./rack/DeviceConditionChips";

interface DeviceHeroCardProps {
  activeDevice: DeviceInfo | SavedDevice | null;
  selectedSerial: string | null;
  displayName: string;
  nickname?: string;
  isLive: boolean;
  isWireless: boolean;
  hasUsb?: boolean;
  hasWifi?: boolean;
  repairMessage?: string | null;
  activeSession: SessionInfo | null;
  currentMode: StudioMode;
  battery?: BatteryInfo;
  overview?: PhoneOverview | null;
  launching: boolean;
  reconnecting: boolean;
  refreshing: boolean;
  onMirror: () => void;
  onReconnect: () => void;
  onRefresh: () => void;
  onOpenPair?: () => void;
  onViewPhoneDetails?: (serial: string) => void;
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
  hasUsb,
  hasWifi,
  repairMessage,
  activeSession,
  currentMode,
  battery,
  overview,
  launching,
  reconnecting,
  refreshing,
  onMirror,
  onReconnect,
  onRefresh,
  onOpenPair,
  onViewPhoneDetails,
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
      <div className="flex items-center justify-between rounded-xl border border-ink-700 bg-ink-900 px-5 py-4">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-ink-700 bg-ink-800 text-text-muted">
            <Smartphone className="h-5 w-5" />
          </div>
          <div>
            <h2 className="text-sm font-semibold text-text-strong">Connect a phone</h2>
            <p className="text-xs text-text-muted">
              Plug in via USB with debugging enabled, or pair wirelessly over Wi-Fi.
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onRefresh}
            disabled={refreshing}
            className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-2.5 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:bg-ink-700 cursor-pointer"
            title="Rescan devices"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin" : ""}`} />
          </button>
          {onOpenPair && (
            <button
              type="button"
              onClick={onOpenPair}
              className="btn-tactile flex items-center gap-1.5 rounded-lg bg-gold-400 px-3.5 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer"
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
  const dualTransport = Boolean(hasUsb && hasWifi);
  const transport: TransportKind = dualTransport
    ? "dual"
    : connectionType === "wifi"
    ? "wifi"
    : "usb";
  const status: DeviceStatus = activeSession
    ? "mirroring"
    : isLive
    ? "connected"
    : "offline";
  const needsRepairMsg =
    !isLive && (repairMessage || (deviceState === "unauthorized" ? MSG_NEEDS_REPAIR : null));
  const resolvedTitle =
    nickname ||
    ("label" in activeDevice && activeDevice.label) ||
    overview?.model ||
    details?.model ||
    displayName;
  const androidVersion = overview?.androidVersion ?? details?.androidVersion;
  const batteryChip = resolveBatteryChipData(overview, battery);

  return (
    <div className="rounded-xl border border-ink-700 bg-ink-900 px-4 py-3.5 space-y-2.5">
      <div className="flex flex-wrap items-center justify-between gap-4">
        {/* Left: Identity & Compact Condition Chips */}
        <div className="flex items-center gap-3.5 min-w-0">
          <div
            className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border ${
              isLive
                ? "border-status-success/40 bg-status-success-bg text-status-success"
                : "border-ink-700 bg-ink-800 text-text-muted"
            }`}
          >
            <Smartphone className="h-5 w-5" />
          </div>

          <div className="min-w-0 space-y-1">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="truncate text-sm font-semibold text-text-strong">
                {resolvedTitle}
              </h2>
              {androidVersion && (
                <span className="font-mono text-[11px] text-text-muted">
                  Android {androidVersion}
                </span>
              )}
              <TransportChip transport={transport} />
              <StatusChip status={status} />
            </div>

            <div className="flex flex-wrap items-center gap-2 font-mono text-[11px] text-text-muted">
              <span className="truncate">{selectedSerial ?? "—"}</span>
              {isLive ? (
                <>
                  <span className="text-ink-700">·</span>
                  <BatteryChip battery={batteryChip} showUnknown={true} />
                  {selectedSerial && (
                    <>
                      <span className="text-ink-700">·</span>
                      <button
                        type="button"
                        onClick={() => onViewPhoneDetails?.(selectedSerial)}
                        className="btn-tactile inline-flex items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-0.5 font-sans text-[11px] font-medium text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
                      >
                        <span>View phone details</span>
                        <ArrowRight className="h-3 w-3" />
                      </button>
                    </>
                  )}
                </>
              ) : (
                <>
                  <span className="text-ink-700">·</span>
                  <span>Phone details need a connection</span>
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
              className="btn-tactile flex items-center gap-1.5 rounded-lg bg-status-danger-solid px-3 py-1.5 text-xs font-semibold text-text-strong hover:opacity-90 cursor-pointer disabled:opacity-50"
            >
              <Square className="h-3 w-3 fill-current" />
              <span>Stop</span>
            </button>
          ) : (
            <button
              type="button"
              disabled={launching || !selectedSerial}
              onClick={onMirror}
              className="btn-tactile flex items-center gap-1.5 rounded-lg bg-gold-400 px-3 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <Play className="h-3 w-3 fill-current" />
              <span>{MODE_LABELS[currentMode]}</span>
            </button>
          )}

          {isWireless && (
            <button
              type="button"
              disabled={reconnecting}
              onClick={onReconnect}
              className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-2.5 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong cursor-pointer disabled:opacity-50"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${reconnecting ? "animate-spin" : ""}`} />
              <span>Reconnect</span>
            </button>
          )}

          <button
            type="button"
            onClick={() => openDoctor(selectedSerial)}
            className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-2.5 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
          >
            <Stethoscope className="h-3.5 w-3.5 text-mist-300" />
            <span>Troubleshoot</span>
          </button>

          <button
            type="button"
            onClick={onRefresh}
            disabled={refreshing}
            className="btn-tactile flex items-center justify-center rounded-lg border border-ink-700 bg-ink-800 p-1.5 text-text-muted hover:border-text-muted hover:text-text-strong cursor-pointer"
            title="Rescan devices"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin text-text-strong" : ""}`} />
          </button>
        </div>
      </div>

      {needsRepairMsg && (
        <div className="flex items-center justify-between gap-3 rounded-lg border border-status-danger/40 bg-status-danger-bg px-3 py-2 text-xs text-status-danger">
          <span>{needsRepairMsg}</span>
          {onOpenPair && (
            <button
              type="button"
              onClick={onOpenPair}
              className="btn-tactile shrink-0 rounded bg-gold-400 px-2.5 py-1 text-[11px] font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer"
            >
              Pair again
            </button>
          )}
        </div>
      )}
    </div>
  );
}
