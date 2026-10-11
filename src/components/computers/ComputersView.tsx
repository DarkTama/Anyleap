import { useEffect, useState } from "react";
import {
  Monitor,
  Power,
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
} from "lucide-react";
import type { DiscoveredPeer } from "@/types/kvm";
import { useKvmStore } from "@/stores/kvmStore";
import { TopologyCanvas } from "./TopologyCanvas";
import { PairingPinModal } from "./PairingPinModal";
import { DesktopHudPill } from "./DesktopHudPill";

export function ComputersView() {
  const status = useKvmStore((s) => s.status);
  const discoveredPeers = useKvmStore((s) => s.discoveredPeers);
  const selectedPeer = useKvmStore((s) => s.selectedPeer);
  const activityFeed = useKvmStore((s) => s.activityFeed);
  const isLoading = useKvmStore((s) => s.isLoading);
  const error = useKvmStore((s) => s.error);
  const init = useKvmStore((s) => s.init);
  const startServer = useKvmStore((s) => s.startServer);
  const stopServer = useKvmStore((s) => s.stopServer);
  const refreshDiscovered = useKvmStore((s) => s.refreshDiscovered);
  const connectPeer = useKvmStore((s) => s.connectPeer);
  const disconnectPeer = useKvmStore((s) => s.disconnectPeer);
  const emergencyRelease = useKvmStore((s) => s.emergencyRelease);
  const openPairingModal = useKvmStore((s) => s.openPairingModal);

  const [manualIp, setManualIp] = useState("192.168.0.202");

  useEffect(() => {
    void init();
    void refreshDiscovered();
    const timer = setInterval(() => {
      void refreshDiscovered();
    }, 2500);
    return () => clearInterval(timer);
  }, [init, refreshDiscovered]);

  const handleManualConnect = async () => {
    const ip = manualIp.trim();
    if (!ip) return;

    let peerName = `PC (${ip})`;
    try {
      const res = await fetch(`http://${ip}:4177/pair`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          computer_id: status.local_id || "local",
          name: status.local_name || "PC",
        }),
      });
      if (res.ok) {
        peerName = `Remote Workstation (${ip})`;
      }
    } catch {
      // fallback
    }

    const peer: DiscoveredPeer = {
      id: `peer-${ip.replace(/[^a-zA-Z0-9]/g, "-")}`,
      name: peerName,
      ip: ip,
      tcp_port: 4177,
      udp_port: 4178,
    };
    void connectPeer(peer);
  };

  return (
    <div className="relative flex h-full flex-1 flex-col overflow-y-auto bg-[#0c0c0e] p-6 text-zinc-100 space-y-6 select-none">
      <DesktopHudPill />

      {/* Top Header & Master Host Toggle */}
      <div className="flex items-center justify-between border-b border-[#222226] pb-4">
        <div>
          <h1 className="text-sm font-semibold text-zinc-100 flex items-center gap-2">
            Computer Link (Virtual KVM)
            <span className="rounded bg-cyan-500/10 px-2 py-0.5 font-mono text-[9px] font-bold text-cyan-400 border border-cyan-500/20">
              PHASE 1
            </span>
            {status.local_name && (
              <span className="rounded bg-zinc-800 px-2 py-0.5 font-mono text-[10px] text-zinc-300">
                {status.local_name}
              </span>
            )}
          </h1>
          <p className="mt-0.5 text-xs text-zinc-400">
            Roam mouse and keyboard across multiple computers by gliding across monitor edges.
          </p>
        </div>

        <div className="flex items-center gap-2">
          {status.is_host_running ? (
            <button
              type="button"
              disabled={isLoading}
              onClick={() => void stopServer()}
              className="btn-tactile flex items-center gap-1.5 rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-1.5 text-xs font-medium text-rose-300 hover:bg-rose-500/20 cursor-pointer"
            >
              <Power className="h-3.5 w-3.5" />
              <span>Receiver Running (Port 4177)</span>
            </button>
          ) : (
            <button
              type="button"
              disabled={isLoading}
              onClick={() => void startServer()}
              className="btn-tactile flex items-center gap-1.5 rounded-lg border border-cyan-500/40 bg-cyan-500/10 px-3 py-1.5 text-xs font-semibold text-cyan-300 hover:bg-cyan-500/20 cursor-pointer shadow-sm"
            >
              <Power className="h-3.5 w-3.5" />
              <span>Start KVM Receiver</span>
            </button>
          )}

          <button
            type="button"
            onClick={() => openPairingModal()}
            className="btn-tactile flex items-center gap-1.5 rounded-lg border border-[#27272a] bg-[#16161a] px-3 py-1.5 text-xs font-medium text-zinc-300 hover:border-zinc-600 hover:bg-[#1c1c21] cursor-pointer"
          >
            <KeyRound className="h-3.5 w-3.5 text-zinc-400" />
            <span>PIN: {status.security_pin || "------"}</span>
          </button>
        </div>
      </div>

      {error && (
        <div className="flex items-center gap-2 rounded-xl border border-rose-500/40 bg-rose-950/40 p-3 text-xs text-rose-200">
          <AlertTriangle className="h-4 w-4 shrink-0 text-rose-400" />
          <span>{error}</span>
        </div>
      )}

      {/* Emergency Escape Reminder Pill */}
      <div className="flex items-center justify-between rounded-xl border border-[#222226] bg-[#141417] px-4 py-3 text-xs">
        <div className="flex items-center gap-2">
          <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
          <span className="font-medium text-zinc-300">
            Cursor edge-leap active. Move mouse past edge to jump to adjacent screen; move back to return. Emergency escape: press <kbd className="rounded bg-[#202026] px-1.5 py-0.5 font-mono text-zinc-100 border border-[#27272a]">{useKvmStore.getState().settings.emergencyKeyCombo}</kbd> anytime.
          </span>
        </div>

        <button
          type="button"
          onClick={() => void emergencyRelease()}
          className="btn-tactile rounded-md bg-rose-500/20 text-rose-300 border border-rose-500/40 px-3 py-1 font-mono text-xs hover:bg-rose-500/30 cursor-pointer font-bold"
        >
          Release Cursor Now
        </button>
      </div>

      {/* 2D Topology Canvas */}
      <TopologyCanvas />

      {/* Discovered Nearby Computers Section */}
      <div className="rounded-xl border border-[#222226] bg-[#141417] p-5 space-y-4">
        <div className="flex items-center justify-between text-xs">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-zinc-400">
            Discovered Nearby Computers (LAN Beacon UDP 4179)
          </span>
          <button
            type="button"
            onClick={() => void refreshDiscovered()}
            className="btn-tactile flex items-center gap-1 text-[11px] text-zinc-400 hover:text-zinc-200 cursor-pointer"
          >
            <RefreshCw className="h-3 w-3" />
            <span>Scan</span>
          </button>
        </div>

        {discoveredPeers.length === 0 ? (
          <div className="rounded-xl border border-dashed border-[#222226] bg-[#0c0c0e] p-6 text-center space-y-3">
            <Monitor className="mx-auto h-8 w-8 text-zinc-600" />
            <div>
              <p className="text-xs font-semibold text-zinc-300">No Nearby Computers Auto-Detected</p>
              <p className="text-[11px] text-zinc-500 mt-1 max-w-md mx-auto leading-relaxed">
                Connect directly using the remote computer's local LAN IP:
              </p>
            </div>

            {/* Manual Direct IP Form */}
            <div className="flex items-center justify-center gap-2 pt-2 max-w-sm mx-auto">
              <input
                type="text"
                placeholder="192.168.0.xxx"
                value={manualIp}
                onChange={(e) => setManualIp(e.target.value)}
                className="h-8 flex-1 rounded-lg border border-[#27272a] bg-[#16161a] px-3 font-mono text-xs text-zinc-200 outline-none focus:border-zinc-500"
              />
              <button
                type="button"
                onClick={() => void handleManualConnect()}
                disabled={!manualIp.trim() || isLoading}
                className="btn-tactile flex items-center gap-1.5 rounded-lg bg-white px-3 py-1.5 text-xs font-semibold text-zinc-950 hover:bg-zinc-200 cursor-pointer disabled:opacity-40"
              >
                <span>Link PC</span>
                <ArrowRight className="h-3 w-3" />
              </button>
            </div>
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-3">
            {discoveredPeers.map((peer) => {
              const isLinked = selectedPeer?.id === peer.id;
              return (
                <div
                  key={peer.id}
                  className={`flex items-center justify-between rounded-xl border p-3.5 ${
                    isLinked
                      ? "border-cyan-500/40 bg-[#161a24]"
                      : "border-[#222226] bg-[#121215]"
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <div className="grid h-9 w-9 place-items-center rounded-lg bg-[#18181c] border border-[#27272a] text-zinc-300">
                      <Monitor className="h-4.5 w-4.5" />
                    </div>
                    <div>
                      <div className="text-xs font-semibold text-zinc-200">{peer.name}</div>
                      <div className="font-mono text-[10px] text-zinc-500">
                        {peer.ip}:{peer.tcp_port}
                      </div>
                    </div>
                  </div>

                  {isLinked ? (
                    <button
                      type="button"
                      onClick={() => void disconnectPeer()}
                      className="btn-tactile rounded-md border border-rose-500/30 bg-rose-500/10 px-3 py-1.5 text-xs font-medium text-rose-300 hover:bg-rose-500/20 cursor-pointer"
                    >
                      Unlink
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => void connectPeer(peer)}
                      className="btn-tactile rounded-md bg-white px-3 py-1.5 text-xs font-semibold text-zinc-950 hover:bg-zinc-200 cursor-pointer"
                    >
                      Link PC
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Live Clipboard & File Activity Feed */}
      <div className="rounded-xl border border-[#222226] bg-[#141417] p-5 space-y-3">
        <div className="flex items-center justify-between text-xs">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-zinc-400">
            Clipboard & File Transfer Activity
          </span>
          <span className="font-mono text-[10px] text-zinc-500">
            {activityFeed.length} event{activityFeed.length === 1 ? "" : "s"}
          </span>
        </div>

        {activityFeed.length === 0 ? (
          <div className="rounded-lg border border-dashed border-[#222226] bg-[#0c0c0e] p-4 text-center text-xs text-zinc-500">
            No clipboard or file sync activity yet. Copy text or images between computers to sync instantly.
          </div>
        ) : (
          <div className="space-y-2 max-h-48 overflow-y-auto pr-1">
            {activityFeed.slice(0, 10).map((act) => {
              const timeStr = new Date(act.timestamp).toLocaleTimeString();
              const isSyncing = act.status === "syncing";

              return (
                <div
                  key={act.id}
                  className="flex items-center justify-between rounded-lg border border-[#202026] bg-[#101013] p-2.5 text-xs"
                >
                  <div className="flex items-center gap-2.5 truncate max-w-[70%]">
                    {act.type === "text" && (
                      <FileText className="h-4 w-4 text-sky-400 shrink-0" />
                    )}
                    {act.type === "image" && (
                      <ImageIcon className="h-4 w-4 text-emerald-400 shrink-0" />
                    )}
                    {act.type === "file" && (
                      <FileIcon className="h-4 w-4 text-amber-400 shrink-0" />
                    )}
                    <div className="flex flex-col truncate">
                      <span className="truncate text-zinc-200 font-mono text-[11px]">
                        {act.name}
                      </span>
                      {act.savedPath && (
                        <span className="truncate text-zinc-500 font-mono text-[9px]">
                          Saved: {act.savedPath}
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="flex items-center gap-3 shrink-0">
                    {isSyncing ? (
                      <div className="flex items-center gap-2">
                        <div className="h-1.5 w-16 rounded-full bg-zinc-800 overflow-hidden">
                          <div
                            className="h-full bg-sky-400 transition-all duration-150"
                            style={{ width: `${act.progressPercent}%` }}
                          />
                        </div>
                        <span className="font-mono text-[10px] text-sky-400">
                          {act.progressPercent}%
                        </span>
                      </div>
                    ) : (
                      <div className="flex items-center gap-1 font-mono text-[10px] text-emerald-400">
                        <CheckCircle2 className="h-3 w-3" />
                        <span>Synced</span>
                      </div>
                    )}
                    <span className="font-mono text-[10px] text-zinc-600">{timeStr}</span>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Feature Badges */}
      <div className="grid grid-cols-2 gap-4 pt-1">
        <div className="flex items-start gap-3 rounded-xl border border-[#222226] bg-[#141417] p-3.5">
          <Zap className="h-4 w-4 shrink-0 text-cyan-400 mt-0.5" />
          <div>
            <div className="text-xs font-semibold text-zinc-200">UDP Mouse Acceleration</div>
            <p className="text-[11px] text-zinc-400 mt-0.5 leading-relaxed">
              Relative mouse deltas bypass TCP buffering on UDP port 4178 for sub-millisecond physical glide.
            </p>
          </div>
        </div>

        <div className="flex items-start gap-3 rounded-xl border border-[#222226] bg-[#141417] p-3.5">
          <Shield className="h-4 w-4 shrink-0 text-emerald-400 mt-0.5" />
          <div>
            <div className="text-xs font-semibold text-zinc-200">Auto-Return Boundary Edge</div>
            <p className="text-[11px] text-zinc-400 mt-0.5 leading-relaxed">
              Moving your cursor back past the boundary edge immediately unhooks and restores local pointer control.
            </p>
          </div>
        </div>
      </div>

      <PairingPinModal />
    </div>
  );
}
