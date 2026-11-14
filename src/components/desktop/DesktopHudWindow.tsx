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

  // This window is only shown on the machine that currently owns the cursor.
  const isRemote = status?.receiver_entered ?? false; // driven by a remote controller
  const activeName = status?.local_name || "This PC";

  return (
    <div className="flex h-screen w-screen items-center justify-center select-none overflow-hidden bg-transparent pointer-events-none">
      <div
        className={`hud-pill flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] pointer-events-none shadow-elevated ${
          isRemote
            ? "border-gold-400 bg-ink-900 text-text-strong"
            : "border-ink-700 bg-ink-900 text-text"
        }`}
      >
        {isRemote ? (
          <Laptop className="h-3 w-3 shrink-0 text-gold-400" />
        ) : (
          <Monitor className="h-3 w-3 shrink-0 text-text-muted" />
        )}
        <span className="truncate font-medium">{activeName}</span>
        {isRemote && (
          <span className="shrink-0 text-[9px] uppercase tracking-wide text-gold-400">remote</span>
        )}
      </div>
    </div>
  );
}
