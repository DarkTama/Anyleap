import { useEffect, useState } from "react";
import { Laptop, Monitor } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { kvmGetStatus } from "@/services/kvmService";
import type { KvmStatus } from "@/types/kvm";

export function DesktopHudWindow() {
  const [status, setStatus] = useState<KvmStatus | null>(null);

  useEffect(() => {
    void kvmGetStatus().then(setStatus).catch(() => {});

    const unlistenConnected = listen<string>("kvm-peer-connected", () => {
      void kvmGetStatus().then(setStatus).catch(() => {});
    });

    const unlistenDisconnected = listen("kvm-peer-disconnected", () => {
      void kvmGetStatus().then(setStatus).catch(() => {});
    });

    const unlistenLeap = listen<string>("kvm-cursor-enter", () => {
      void kvmGetStatus().then(setStatus).catch(() => {});
    });

    const unlistenReturn = listen<string>("kvm-cursor-return", () => {
      void kvmGetStatus().then(setStatus).catch(() => {});
    });

    const timer = setInterval(() => {
      void kvmGetStatus().then(setStatus).catch(() => {});
    }, 1200);

    return () => {
      clearInterval(timer);
      void unlistenConnected.then((f) => f());
      void unlistenDisconnected.then((f) => f());
      void unlistenLeap.then((f) => f());
      void unlistenReturn.then((f) => f());
    };
  }, []);

  const isRemote = status?.is_input_trapped ?? false;
  const peerName = status?.connected_peers?.[0] || "Remote PC";
  const activeName = isRemote ? peerName : status?.local_name || "This PC";

  return (
    <div className="flex h-screen w-screen items-center justify-center p-2 select-none overflow-hidden bg-transparent pointer-events-none">
      <div
        className={`flex items-center gap-2.5 rounded-full border px-3.5 py-1.5 text-xs font-mono shadow-2xl backdrop-blur-md transition-all duration-200 pointer-events-none ${
          isRemote
            ? "border-sky-400 bg-[#0c182b]/95 text-sky-200 shadow-[0_0_24px_rgba(56,189,248,0.45)] ring-1 ring-sky-400/40"
            : "border-zinc-700/80 bg-[#121216]/90 text-zinc-300 shadow-xl"
        }`}
      >
        <span
          className={`h-2.5 w-2.5 rounded-full ${
            isRemote ? "bg-sky-400" : "bg-emerald-400"
          }`}
        />
        {isRemote ? (
          <Laptop className="h-3.5 w-3.5 text-sky-400" />
        ) : (
          <Monitor className="h-3.5 w-3.5 text-zinc-400" />
        )}
        <span className="font-semibold tracking-wide truncate max-w-[130px]">
          {activeName}
        </span>
        <span className="text-[10px] uppercase font-bold text-zinc-400 border-l border-zinc-700/80 pl-2">
          {isRemote ? "REMOTE" : "LOCAL"}
        </span>
      </div>
    </div>
  );
}
