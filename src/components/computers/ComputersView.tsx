import { useEffect, useState } from "react";
import {
  Monitor,
  RefreshCw,
  KeyRound,
  AlertTriangle,
  ArrowRight,
  Shield,
  Zap,
  FileText,
  Image as ImageIcon,
  File as FileIcon,
  CheckCircle2,
  Trash2,
  Unlink,
} from "lucide-react";
import type { DiscoveredPeer } from "@/types/kvm";
import { useKvmStore } from "@/stores/kvmStore";
import { kvmSetSyncSettings } from "@/services/kvmService";
import { TopologyCanvas } from "./TopologyCanvas";
import { PairingPinModal } from "./PairingPinModal";
import { getComputerRole, getVisibleSections } from "./topologyLayout";

export function ComputersView() {
  const status = useKvmStore((s) => s.status);
  const discoveredPeers = useKvmStore((s) => s.discoveredPeers);
  const trustedPeers = useKvmStore((s) => s.trustedPeers);
  const topology = useKvmStore((s) => s.topology);
  const activityFeed = useKvmStore((s) => s.activityFeed);
  const isLoading = useKvmStore((s) => s.isLoading);
  const error = useKvmStore((s) => s.error);
  const init = useKvmStore((s) => s.init);
  const setAllowControl = useKvmStore((s) => s.setAllowControl);
  const refreshDiscovered = useKvmStore((s) => s.refreshDiscovered);
  const refreshTrusted = useKvmStore((s) => s.refreshTrusted);
  const connectPeer = useKvmStore((s) => s.connectPeer);
  const disconnectPeer = useKvmStore((s) => s.disconnectPeer);
  const forgetPeer = useKvmStore((s) => s.forgetPeer);
  const emergencyRelease = useKvmStore((s) => s.emergencyRelease);
  const openPairingModal = useKvmStore((s) => s.openPairingModal);

  const [manualIp, setManualIp] = useState("");

  useEffect(() => {
    const { syncClipboardText, syncClipboardImage, syncFileTransfer } =
      useKvmStore.getState().settings;
    void kvmSetSyncSettings(
      syncClipboardText,
      syncClipboardImage,
      syncFileTransfer
    ).catch(() => {});
    void init();
    void refreshDiscovered();
    void refreshTrusted();
    const timer = setInterval(() => {
      void refreshDiscovered();
    }, 2500);
    return () => clearInterval(timer);
  }, [init, refreshDiscovered, refreshTrusted]);

  const handleConnectPeer = async (peer: DiscoveredPeer) => {
    await connectPeer(peer);
    if (useKvmStore.getState().error?.includes("not paired")) {
      openPairingModal(peer);
    }
  };

  const handleManualConnect = async () => {
    const ip = manualIp.trim();
    if (!ip) return;

    const peer: DiscoveredPeer = {
      id: `peer-${ip.replace(/[^a-zA-Z0-9]/g, "-")}`,
      name: `PC (${ip})`,
      ip,
      port: 4177,
    };
    await handleConnectPeer(peer);
  };

  const role = getComputerRole(status);
  const sections = getVisibleSections(role);
  const allowControl = status.allow_control ?? status.is_host_running;

  const remotePeer = topology.computers.find((c) => !c.is_local);
  const peerName =
    status.linked_peer?.name ||
    remotePeer?.name ||
    status.connected_peers[0] ||
    "the other PC";
  const edge = status.controller_edge;

  const [linkTone, linkMessage] =
    role === "main"
      ? status.is_input_trapped
        ? ["bg-gold-400", `Controlling ${peerName} now.`]
        : [
            "bg-status-success",
            `Linked to ${peerName} as Main PC. Move your cursor past the ${(edge || "right").toUpperCase()} edge to control it.`,
          ]
      : [
          "bg-text-muted",
          allowControl
            ? "Ready to link. Click Link on another PC to control it, or let another PC link to this one."
            : "Not linked. Turn on 'Allow other PCs to control this PC' if another PC will link to this one.",
        ];

  return (
    <div className="relative flex h-full flex-1 flex-col overflow-y-auto bg-ink-950 p-6 text-text space-y-6 select-none">
      {/* Top Header */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-ink-700 pb-4">
        <div>
          <h1 className="text-sm font-semibold text-text-strong flex items-center gap-2">
            Shared Mouse &amp; Keyboard
            {status.local_name && (
              <span className="rounded bg-ink-800 px-2 py-0.5 font-mono text-[10px] text-text border border-ink-700">
                {status.local_name}
              </span>
            )}
            {role === "main" && (
              <span className="rounded bg-ink-800 border border-mist-300/40 px-2 py-0.5 font-mono text-[10px] text-mist-300">
                Main PC
              </span>
            )}
            {role === "linked" && (
              <span className="rounded bg-status-success-bg border border-status-success/40 px-2 py-0.5 font-mono text-[10px] text-status-success">
                Linked PC
              </span>
            )}
          </h1>
          <p className="mt-0.5 text-xs text-text-muted">
            Share your mouse and keyboard with another PC by moving the cursor across screen edges.
          </p>
        </div>

        <div className="flex items-center gap-2">
          {sections.showAllowControlToggle && (
            <label className="flex items-center gap-2 rounded-lg border border-ink-700 bg-ink-900 px-3 py-1.5 text-xs text-text cursor-pointer">
              <input
                type="checkbox"
                checked={allowControl}
                disabled={isLoading}
                onChange={(e) => void setAllowControl(e.target.checked)}
                className="accent-gold-400 cursor-pointer"
              />
              <span>Allow other PCs to control this PC</span>
            </label>
          )}

          {sections.showPairingCode && (
            <button
              type="button"
              onClick={() => openPairingModal()}
              className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-900 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:bg-ink-800 cursor-pointer"
            >
              <KeyRound className="h-3.5 w-3.5 text-gold-400" />
              <span>
                {allowControl && status.security_pin
                  ? `Pairing code: ${status.security_pin}`
                  : "Pairing code"}
              </span>
            </button>
          )}

          {sections.showUnlink && (
            <button
              type="button"
              disabled={isLoading}
              onClick={() => void disconnectPeer()}
              className="btn-tactile flex items-center gap-1.5 rounded-lg border border-status-danger/40 bg-status-danger-bg px-3 py-1.5 text-xs font-semibold text-status-danger hover:opacity-90 cursor-pointer disabled:opacity-40"
            >
              <Unlink className="h-3.5 w-3.5" />
              <span>Unlink</span>
            </button>
          )}
        </div>
      </div>

      {error && (
        <div className="flex items-center gap-2 rounded-xl border border-status-danger/40 bg-status-danger-bg p-3 text-xs text-status-danger">
          <AlertTriangle className="h-4 w-4 shrink-0 text-status-danger" />
          <span>{error}</span>
        </div>
      )}

      {/* Status Card (Not linked & Main PC) */}
      {sections.showStatusCard && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-ink-700 bg-ink-900 px-4 py-3 text-xs">
          <div className="flex items-center gap-2">
            <span className={`h-2 w-2 shrink-0 rounded-full ${linkTone}`} />
            <span className="font-medium text-text">
              {linkMessage}
              {role === "main" && (
                <>
                  {" "}
                  Emergency escape:{" "}
                  <kbd className="rounded bg-ink-800 px-1.5 py-0.5 font-mono text-text-strong border border-ink-700">
                    Scroll Lock, Pause, or Ctrl+Alt+Esc
                  </kbd>
                  .
                </>
              )}
            </span>
          </div>

          {sections.showBringCursorBack && (
            <button
              type="button"
              onClick={() => void emergencyRelease()}
              className="btn-tactile rounded-md bg-status-danger-solid text-text-strong px-3 py-1 font-mono text-xs hover:opacity-90 cursor-pointer font-bold"
            >
              Bring cursor back
            </button>
          )}
        </div>
      )}

      {/* Linked PC Card ("Controlled by <name>") */}
      {sections.showControlledByCard && (
        <div className="flex flex-wrap items-center justify-between gap-4 rounded-xl border border-status-success/40 bg-status-success-bg p-5">
          <div className="flex items-center gap-3">
            <span
              className={`h-2.5 w-2.5 shrink-0 rounded-full ${
                status.receiver_entered
                  ? "bg-gold-400"
                  : "bg-status-success"
              }`}
            />
            <div>
              <div className="text-sm font-semibold text-text-strong">
                Controlled by {peerName}
              </div>
              <p className="text-xs text-text-muted mt-0.5">
                {status.receiver_entered
                  ? `${peerName} is currently controlling this PC's mouse and keyboard.`
                  : `${peerName} is linked and can move its cursor onto this PC.`}
              </p>
            </div>
          </div>

          <button
            type="button"
            disabled={isLoading}
            onClick={() => void disconnectPeer()}
            className="btn-tactile flex items-center gap-1.5 rounded-lg border border-status-danger/40 bg-status-danger-bg px-3.5 py-1.5 text-xs font-semibold text-status-danger hover:opacity-90 cursor-pointer disabled:opacity-40"
          >
            <Unlink className="h-3.5 w-3.5" />
            <span>Unlink</span>
          </button>
        </div>
      )}

      {/* 2D Scaled Layout Canvas (Main PC only) */}
      {sections.showLayoutCanvas && <TopologyCanvas />}

      {/* Discovered Nearby Computers (Not linked only) */}
      {sections.showDiscoveredPeers && (
        <div className="rounded-xl border border-ink-700 bg-ink-900 p-5 space-y-4">
          <div className="flex items-center justify-between text-xs">
            <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
              Nearby Computers (LAN Beacon UDP 4179)
            </span>
            <button
              type="button"
              onClick={() => void refreshDiscovered()}
              className="btn-tactile flex items-center gap-1 text-[11px] text-text-muted hover:text-text-strong cursor-pointer"
            >
              <RefreshCw className="h-3 w-3" />
              <span>Scan</span>
            </button>
          </div>

          {discoveredPeers.length === 0 ? (
            <div className="rounded-xl border border-dashed border-ink-700 bg-ink-950 p-6 text-center space-y-3">
              <Monitor className="mx-auto h-8 w-8 text-text-muted" />
              <div>
                <p className="text-xs font-semibold text-text-strong">
                  No Nearby Computers Auto-Detected
                </p>
                <p className="text-[11px] text-text-muted mt-1 max-w-md mx-auto leading-relaxed">
                  Link directly using the other PC&apos;s local LAN IP:
                </p>
              </div>

              <div className="flex items-center justify-center gap-2 pt-2 max-w-sm mx-auto">
                <input
                  type="text"
                  placeholder="192.168.x.x"
                  value={manualIp}
                  onChange={(e) => setManualIp(e.target.value)}
                  className="h-8 flex-1 rounded-lg border border-ink-700 bg-ink-800 px-3 font-mono text-xs text-text-strong outline-none focus:border-gold-400"
                />
                <button
                  type="button"
                  onClick={() => void handleManualConnect()}
                  disabled={!manualIp.trim() || isLoading}
                  className="btn-tactile flex items-center gap-1.5 rounded-lg bg-gold-400 px-3 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer disabled:opacity-40"
                >
                  <span>Link</span>
                  <ArrowRight className="h-3 w-3" />
                </button>
              </div>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-3">
              {discoveredPeers.map((peer) => (
                <div
                  key={peer.id}
                  className="flex items-center justify-between rounded-xl border border-ink-700 bg-ink-950 p-3.5"
                >
                  <div className="flex items-center gap-3">
                    <div className="grid h-9 w-9 place-items-center rounded-lg bg-ink-800 border border-ink-700 text-text">
                      <Monitor className="h-4.5 w-4.5" />
                    </div>
                    <div>
                      <div className="text-xs font-semibold text-text-strong">
                        {peer.name}
                      </div>
                      <div className="font-mono text-[10px] text-text-muted">
                        {peer.ip}:{peer.port}
                      </div>
                    </div>
                  </div>

                  <button
                    type="button"
                    disabled={isLoading}
                    onClick={() => void handleConnectPeer(peer)}
                    className="btn-tactile rounded-md bg-gold-400 px-3 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer disabled:opacity-40"
                  >
                    Link
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Trusted PCs List (Linked PC only) */}
      {sections.showTrustedPeers && (
        <div className="rounded-xl border border-ink-700 bg-ink-900 p-5 space-y-3">
          <div className="flex items-center justify-between text-xs">
            <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
              Trusted PCs
            </span>
            <button
              type="button"
              onClick={() => void refreshTrusted()}
              className="btn-tactile flex items-center gap-1 text-[11px] text-text-muted hover:text-text-strong cursor-pointer"
            >
              <RefreshCw className="h-3 w-3" />
              <span>Refresh</span>
            </button>
          </div>

          {trustedPeers.length === 0 ? (
            <div className="rounded-lg border border-dashed border-ink-700 bg-ink-950 p-4 text-center text-xs text-text-muted">
              No trusted PCs saved yet.
            </div>
          ) : (
            <div className="space-y-2">
              {trustedPeers.map((tp) => (
                <div
                  key={tp.computer_id}
                  className="flex items-center justify-between rounded-lg border border-ink-700 bg-ink-950 p-3 text-xs"
                >
                  <div>
                    <div className="font-semibold text-text-strong">{tp.name}</div>
                    <div className="font-mono text-[10px] text-text-muted">
                      {tp.address || tp.computer_id}
                    </div>
                  </div>
                  <button
                    type="button"
                    disabled={isLoading}
                    onClick={() => void forgetPeer(tp.computer_id)}
                    className="btn-tactile flex items-center gap-1 rounded-md border border-status-danger/40 bg-status-danger-bg px-2.5 py-1 text-xs font-medium text-status-danger hover:opacity-90 cursor-pointer disabled:opacity-40"
                  >
                    <Trash2 className="h-3 w-3" />
                    <span>Forget</span>
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Live Clipboard & File Activity Feed */}
      <div className="rounded-xl border border-ink-700 bg-ink-900 p-5 space-y-3">
        <div className="flex items-center justify-between text-xs">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
            Clipboard &amp; File Transfer Activity
          </span>
          <span className="font-mono text-[10px] text-text-muted">
            {activityFeed.length} event{activityFeed.length === 1 ? "" : "s"}
          </span>
        </div>

        {activityFeed.length === 0 ? (
          <div className="rounded-lg border border-dashed border-ink-700 bg-ink-950 p-4 text-center text-xs text-text-muted">
            No clipboard or file sync activity yet.
          </div>
        ) : (
          <div className="space-y-2 max-h-48 overflow-y-auto pr-1">
            {activityFeed.slice(0, 10).map((act) => {
              const timeStr = new Date(act.timestamp).toLocaleTimeString();
              const isSyncing = act.status === "syncing";

              return (
                <div
                  key={act.id}
                  className="flex items-center justify-between rounded-lg border border-ink-700 bg-ink-950 p-2.5 text-xs"
                >
                  <div className="flex items-center gap-2.5 truncate max-w-[70%]">
                    {act.type === "text" && (
                      <FileText className="h-4 w-4 text-mist-300 shrink-0" />
                    )}
                    {act.type === "image" && (
                      <ImageIcon className="h-4 w-4 text-status-success shrink-0" />
                    )}
                    {act.type === "file" && (
                      <FileIcon className="h-4 w-4 text-status-warning shrink-0" />
                    )}
                    <div className="flex flex-col truncate">
                      <span className="truncate text-text-strong font-mono text-[11px]">
                        {act.name}
                      </span>
                      {act.savedPath && (
                        <span className="truncate text-text-muted font-mono text-[9px]">
                          Saved: {act.savedPath}
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-3 shrink-0">
                    {isSyncing ? (
                      <div className="flex items-center gap-2">
                        <div className="h-1.5 w-16 rounded-full bg-ink-800 overflow-hidden">
                          <div
                            className="h-full bg-gold-400 transition-[width] duration-150 ease-out"
                            style={{ width: `${act.progressPercent}%` }}
                          />
                        </div>
                        <span className="font-mono text-[10px] text-gold-400">
                          {act.progressPercent}%
                        </span>
                      </div>
                    ) : (
                      <div className="flex items-center gap-1 font-mono text-[10px] text-status-success">
                        <CheckCircle2 className="h-3 w-3" />
                        <span>Synced</span>
                      </div>
                    )}
                    <span className="font-mono text-[10px] text-text-muted">
                      {timeStr}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Feature Badges */}
      <div className="grid grid-cols-2 gap-4 pt-1">
        <div className="flex items-start gap-3 rounded-xl border border-ink-700 bg-ink-900 p-3.5">
          <Zap className="h-4 w-4 shrink-0 text-gold-400 mt-0.5" />
          <div>
            <div className="text-xs font-semibold text-text-strong">
              Low-latency pointer (QUIC datagrams)
            </div>
            <p className="text-[11px] text-text-muted mt-0.5 leading-relaxed">
              Relative mouse deltas travel as unreliable QUIC datagrams so bulk transfers never delay the cursor.
            </p>
          </div>
        </div>

        <div className="flex items-start gap-3 rounded-xl border border-ink-700 bg-ink-900 p-3.5">
          <Shield className="h-4 w-4 shrink-0 text-status-success mt-0.5" />
          <div>
            <div className="text-xs font-semibold text-text-strong">
              Auto-Return Boundary Edge
            </div>
            <p className="text-[11px] text-text-muted mt-0.5 leading-relaxed">
              Moving your cursor back past the boundary edge immediately unhooks and restores local pointer control.
            </p>
          </div>
        </div>
      </div>

      <PairingPinModal />
    </div>
  );
}
