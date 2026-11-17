import { Check, Pencil, X } from "lucide-react";
import type { DeviceStatus } from "@/lib/deviceStatus";
import {
  DeviceConnectionControls,
  type QuickActionState,
} from "./DeviceConnectionControls";
import {
  BatteryChip,
  StatusChip,
  TransportChip,
  type TransportKind,
} from "./DeviceConditionChips";

export type { TransportKind };

interface DeviceListItemProps {
  title: string;
  subtitle: string;
  telemetryLabel: string;
  status: DeviceStatus;
  reconnecting?: boolean;
  offlineTone?: "muted" | "danger";
  isSelected: boolean;
  isOfflineDimmed?: boolean;
  transport: TransportKind;
  battery: { level: number | null; charging: boolean } | null;
  repairMsg?: string | null;
  isEditing: boolean;
  editingName: string;
  serial?: string;
  hasSession: boolean;
  isBusy: boolean;
  showDoctor: boolean;
  showDisconnect: boolean;
  canQuickAction: boolean;
  canForget: boolean;
  quickAction: QuickActionState | null;
  quickInput: string;
  quickNote: string | null;
  onSelect: () => void;
  onStartEdit: () => void;
  onEditingNameChange: (val: string) => void;
  onSaveEdit: () => void;
  onCancelEdit: () => void;
  onOpenPair?: () => void;
  onConnect?: () => void;
  onStop: () => void;
  onLaunch: () => void;
  onDoctor: () => void;
  onDisconnect?: () => void;
  onOpenQuickInput: (kind: "url" | "call") => void;
  onQuickInputChange: (val: string) => void;
  onSubmitQuickInput: () => void;
  onCloseQuickInput: () => void;
  onForget?: () => void;
}

/**
 * Single device card inside the left-hand device rack.
 * Displays identity, compact condition chips (transport, battery %, status),
 * and connection/mirroring controls.
 */
export function DeviceListItem({
  title,
  subtitle,
  telemetryLabel,
  status,
  reconnecting = false,
  offlineTone = "muted",
  isSelected,
  isOfflineDimmed = false,
  transport,
  battery,
  repairMsg,
  isEditing,
  editingName,
  serial,
  hasSession,
  isBusy,
  showDoctor,
  showDisconnect,
  canQuickAction,
  canForget,
  quickAction,
  quickInput,
  quickNote,
  onSelect,
  onStartEdit,
  onEditingNameChange,
  onSaveEdit,
  onCancelEdit,
  onOpenPair,
  onConnect,
  onStop,
  onLaunch,
  onDoctor,
  onDisconnect,
  onOpenQuickInput,
  onQuickInputChange,
  onSubmitQuickInput,
  onCloseQuickInput,
  onForget,
}: DeviceListItemProps) {
  const dotClass =
    status === "mirroring"
      ? "bg-gold-400"
      : status === "connected"
      ? "bg-status-success"
      : offlineTone === "danger"
      ? "bg-status-danger"
      : "bg-text-muted";

  const containerToneClass = isSelected
    ? "border-gold-400 bg-ink-800"
    : isOfflineDimmed && status === "offline"
    ? "border-ink-700/60 bg-ink-950/40 opacity-75 hover:opacity-100 hover:border-text-muted"
    : "border-ink-700 bg-ink-950/60 text-text hover:border-text-muted hover:bg-ink-800/60";

  return (
    <div
      onClick={onSelect}
      className={`btn-tactile group relative rounded-xl border p-3 cursor-pointer ${containerToneClass}`}
    >
      {/* Header: Status Beacon & Hardware Identity */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <div className={`h-2 w-2 rounded-full shrink-0 ${dotClass}`} />
          <div className="min-w-0">
            <span className="font-semibold text-xs text-text-strong truncate block">
              {title}
            </span>

            {isEditing ? (
              <div
                className="mt-1 flex items-center gap-1"
                onClick={(e) => e.stopPropagation()}
              >
                <input
                  type="text"
                  className="h-6 rounded bg-ink-800 px-1.5 text-xs text-text-strong border border-gold-400 outline-none w-28"
                  value={editingName}
                  autoFocus
                  onChange={(e) => onEditingNameChange(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") onSaveEdit();
                    if (e.key === "Escape") onCancelEdit();
                  }}
                />
                <button
                  type="button"
                  onClick={onSaveEdit}
                  className="btn-tactile text-gold-400 hover:text-text-strong p-0.5 cursor-pointer"
                >
                  <Check className="h-3 w-3" />
                </button>
                <button
                  type="button"
                  onClick={onCancelEdit}
                  className="btn-tactile text-text-muted hover:text-text p-0.5 cursor-pointer"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            ) : (
              <div className="flex items-center gap-1 mt-0.5">
                <span
                  className={`text-[11px] truncate ${
                    isSelected ? "text-text" : "text-text-muted"
                  }`}
                >
                  {subtitle}
                  {telemetryLabel && telemetryLabel !== subtitle
                    ? ` · ${telemetryLabel}`
                    : ""}
                </span>
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onStartEdit();
                  }}
                  className="hover-reveal text-text-muted hover:text-text-strong cursor-pointer"
                  aria-label="Edit nickname"
                >
                  <Pencil className="h-2.5 w-2.5" />
                </button>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Compact Condition Chips: Transport, Battery %, Status */}
      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        <TransportChip transport={transport} />
        <BatteryChip battery={battery} showUnknown={true} />
        <StatusChip status={status} reconnecting={reconnecting} />
      </div>

      {repairMsg && (
        <div
          className="mt-2 flex items-center justify-between gap-2 rounded-md border border-status-danger/40 bg-status-danger-bg px-2 py-1 text-[10px] text-status-danger"
          onClick={(e) => e.stopPropagation()}
        >
          <span className="truncate">{repairMsg}</span>
          {onOpenPair && (
            <button
              type="button"
              onClick={onOpenPair}
              className="btn-tactile shrink-0 rounded bg-gold-400 px-1.5 py-0.5 text-[10px] font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer"
            >
              Pair again
            </button>
          )}
        </div>
      )}

      <DeviceConnectionControls
        serial={serial}
        status={status}
        hasSession={hasSession}
        isBusy={isBusy}
        showDoctor={showDoctor}
        showDisconnect={showDisconnect}
        canQuickAction={canQuickAction}
        canForget={canForget}
        quickAction={quickAction}
        quickInput={quickInput}
        quickNote={quickNote}
        onConnect={onConnect}
        onStop={onStop}
        onLaunch={onLaunch}
        onDoctor={onDoctor}
        onDisconnect={onDisconnect}
        onOpenQuickInput={onOpenQuickInput}
        onQuickInputChange={onQuickInputChange}
        onSubmitQuickInput={onSubmitQuickInput}
        onCloseQuickInput={onCloseQuickInput}
        onForget={onForget}
      />
    </div>
  );
}
