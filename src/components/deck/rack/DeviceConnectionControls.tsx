import type React from "react";
import {
  Link2,
  Phone,
  Play,
  RefreshCw,
  Send,
  Square,
  Stethoscope,
  Trash2,
  Unplug,
} from "lucide-react";
import type { DeviceStatus } from "@/lib/deviceStatus";

export interface QuickActionState {
  serial: string;
  kind: "url" | "call";
}

interface DeviceConnectionControlsProps {
  serial?: string;
  status: DeviceStatus;
  hasSession: boolean;
  isBusy: boolean;
  showDoctor: boolean;
  showDisconnect: boolean;
  canQuickAction: boolean;
  canForget: boolean;
  quickAction: QuickActionState | null;
  quickInput: string;
  quickNote: string | null;
  onConnect?: () => void;
  onStop: () => void;
  onLaunch: () => void;
  onDoctor: () => void;
  onDisconnect?: () => void;
  onOpenQuickInput: (kind: "url" | "call") => void;
  onQuickInputChange: (value: string) => void;
  onSubmitQuickInput: () => void;
  onCloseQuickInput: () => void;
  onForget?: () => void;
}

/**
 * Primary connection/mirror action buttons and quick URL/call popover for a rack device item.
 */
export function DeviceConnectionControls({
  serial,
  status,
  hasSession,
  isBusy,
  showDoctor,
  showDisconnect,
  canQuickAction,
  canForget,
  quickAction,
  quickInput,
  quickNote,
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
}: DeviceConnectionControlsProps) {
  const isQuickOpen = Boolean(serial && quickAction?.serial === serial);

  return (
    <>
      <div
        className="mt-2.5 flex items-center gap-1.5"
        onClick={(e) => e.stopPropagation()}
      >
        {status === "offline" && onConnect ? (
          <button
            type="button"
            disabled={isBusy}
            onClick={onConnect}
            className="btn-tactile flex-1 flex items-center justify-center gap-1 rounded-md bg-ink-800 py-1 text-[11px] font-medium text-text-strong border border-ink-700 hover:border-text-muted cursor-pointer"
          >
            <RefreshCw className={`h-3 w-3 ${isBusy ? "animate-spin" : ""}`} />
            Connect
          </button>
        ) : status === "mirroring" && hasSession ? (
          <button
            type="button"
            disabled={isBusy}
            onClick={onStop}
            className="btn-tactile flex-1 flex items-center justify-center gap-1 rounded-md bg-status-danger-bg py-1 text-[11px] font-medium text-status-danger border border-status-danger/40 hover:opacity-90 cursor-pointer"
          >
            <Square className="h-3 w-3 fill-current" />
            Stop
          </button>
        ) : (
          <button
            type="button"
            disabled={isBusy}
            onClick={onLaunch}
            className="btn-tactile flex-1 flex items-center justify-center gap-1 rounded-md bg-ink-800 py-1 text-[11px] font-medium text-text-strong border border-ink-700 hover:border-text-muted cursor-pointer"
          >
            <Play className="h-3 w-3 fill-current" />
            Launch
          </button>
        )}

        {showDoctor && (
          <button
            type="button"
            onClick={onDoctor}
            className="btn-tactile rounded-md bg-status-warning-bg p-1 text-status-warning border border-status-warning/40 hover:opacity-90 cursor-pointer"
            title="Run Connection Doctor"
          >
            <Stethoscope className="h-3.5 w-3.5" />
          </button>
        )}

        {canQuickAction && (
          <>
            {showDisconnect && onDisconnect && (
              <button
                type="button"
                disabled={isBusy}
                onClick={onDisconnect}
                className="btn-tactile rounded-md bg-ink-800 p-1 text-text hover:text-text-strong border border-ink-700 hover:border-text-muted cursor-pointer"
                title="Disconnect wireless"
              >
                <Unplug className="h-3.5 w-3.5" />
              </button>
            )}
            <button
              type="button"
              disabled={isBusy}
              onClick={() => onOpenQuickInput("url")}
              className={`btn-tactile rounded-md p-1 border cursor-pointer ${
                isQuickOpen && quickAction?.kind === "url"
                  ? "bg-ink-800 text-gold-400 border-gold-400"
                  : "bg-ink-800 text-text border-ink-700 hover:text-text-strong hover:border-text-muted"
              }`}
              title="Open link on phone"
            >
              <Link2 className="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              disabled={isBusy}
              onClick={() => onOpenQuickInput("call")}
              className={`btn-tactile rounded-md p-1 border cursor-pointer ${
                isQuickOpen && quickAction?.kind === "call"
                  ? "bg-ink-800 text-gold-400 border-gold-400"
                  : "bg-ink-800 text-text border-ink-700 hover:text-text-strong hover:border-text-muted"
              }`}
              title="Call…"
            >
              <Phone className="h-3.5 w-3.5" />
            </button>
          </>
        )}

        {canForget && onForget && (
          <button
            type="button"
            disabled={isBusy}
            onClick={onForget}
            className="btn-tactile rounded-md bg-ink-800 p-1 text-text border border-ink-700 hover:text-status-danger hover:border-status-danger/40 cursor-pointer"
            title="Forget device"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        )}
      </div>

      {isQuickOpen && quickAction && (
        <div
          className="popover-panel mt-2 flex items-center gap-1"
          style={{ "--transform-origin": "top right" } as React.CSSProperties}
          onClick={(e) => e.stopPropagation()}
        >
          <input
            type={quickAction.kind === "call" ? "tel" : "url"}
            value={quickInput}
            autoFocus
            onChange={(e) => onQuickInputChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") onSubmitQuickInput();
              if (e.key === "Escape") onCloseQuickInput();
            }}
            placeholder={quickAction.kind === "url" ? "https://…" : "+1 555 123 4567"}
            className="h-7 min-w-0 flex-1 rounded-md border border-ink-700 bg-ink-800 px-2 text-[11px] text-text-strong placeholder:text-text-muted focus:border-gold-400 focus:outline-none"
          />
          <button
            type="button"
            disabled={isBusy || !quickInput.trim()}
            onClick={onSubmitQuickInput}
            className="btn-tactile rounded-md border border-gold-400 bg-gold-400 p-1.5 text-ink-950 hover:bg-gold-600 disabled:opacity-40 cursor-pointer"
            title={quickAction.kind === "url" ? "Open on phone" : "Place call"}
          >
            <Send className="h-3 w-3" />
          </button>
        </div>
      )}

      {quickNote && (
        <div className="mt-1.5 text-[10px] text-status-warning">{quickNote}</div>
      )}
    </>
  );
}
