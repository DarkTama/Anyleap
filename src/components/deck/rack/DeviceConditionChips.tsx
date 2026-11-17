import { Battery, BatteryCharging, Usb, Wifi } from "lucide-react";
import type { DeviceStatus } from "@/lib/deviceStatus";

export type TransportKind = "usb" | "wifi" | "dual";

export function TransportChip({ transport }: { transport: TransportKind }) {
  return (
    <span className="inline-flex items-center gap-1 rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 font-mono text-[11px] leading-[14px] text-text whitespace-nowrap">
      {transport === "dual" ? (
        <>
          <Usb className="h-3 w-3 text-status-success" />
          <span>USB + Wireless</span>
        </>
      ) : transport === "usb" ? (
        <>
          <Usb className="h-3 w-3 text-text" />
          <span>USB</span>
        </>
      ) : (
        <>
          <Wifi className="h-3 w-3 text-mist-300" />
          <span>Wireless</span>
        </>
      )}
    </span>
  );
}

export function BatteryChip({
  battery,
  showUnknown = false,
}: {
  battery: { level: number | null; charging: boolean } | null;
  showUnknown?: boolean;
}) {
  if (!battery || battery.level === null || battery.level === undefined) {
    if (!showUnknown) return null;
    return (
      <span
        aria-label="Battery unknown"
        className="inline-flex items-center gap-1 rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 font-mono text-[11px] leading-[14px] text-text whitespace-nowrap"
      >
        <Battery className="h-3 w-3 text-text" />
        <span>—</span>
      </span>
    );
  }

  const isLow = battery.level <= 20;
  return (
    <span
      aria-label={`Battery ${battery.level} percent${battery.charging ? ", charging" : ""}`}
      className={`inline-flex items-center gap-1 rounded border px-1.5 py-0.5 font-mono text-[11px] leading-[14px] whitespace-nowrap ${
        isLow
          ? "border-transparent bg-status-warning-bg text-status-warning"
          : "border-ink-700 bg-ink-800 text-text"
      }`}
    >
      {battery.charging ? (
        <BatteryCharging
          className={`h-3 w-3 ${isLow ? "text-status-warning" : "text-status-success"}`}
        />
      ) : (
        <Battery className={`h-3 w-3 ${isLow ? "text-status-warning" : "text-text"}`} />
      )}
      <span>{battery.level}%</span>
    </span>
  );
}

export function StatusChip({
  status,
  reconnecting = false,
}: {
  status: DeviceStatus;
  reconnecting?: boolean;
}) {
  const warm = status === "mirroring" || (status === "offline" && reconnecting);
  const styles =
    warm
      ? "border-ink-700 bg-ink-800 text-gold-400"
      : status === "connected"
      ? "border-transparent bg-status-success-bg text-status-success"
      : "border-transparent bg-status-danger-bg text-status-danger";

  const dot =
    warm
      ? "bg-gold-400"
      : status === "connected"
      ? "bg-status-success"
      : "bg-status-danger";

  const label =
    status === "offline" && reconnecting
      ? "Reconnecting…"
      : status === "mirroring" ? "Mirroring" : status === "connected" ? "Connected" : "Offline";

  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded border px-1.5 py-0.5 font-mono text-[11px] leading-[14px] whitespace-nowrap ${styles}`}
    >
      <span className={`h-2 w-2 rounded-full ${dot}`} />
      <span>{label}</span>
    </span>
  );
}
