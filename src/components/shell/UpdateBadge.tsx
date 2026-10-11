import { Sparkles, ArrowDownCircle } from "lucide-react";
import { useAppUpdateStore } from "@/stores/appUpdateStore";

export function UpdateBadge() {
  const hasUpdate = useAppUpdateStore((s) => s.hasUpdate);
  const dismissed = useAppUpdateStore((s) => s.dismissed);
  const availableVersion = useAppUpdateStore((s) => s.availableVersion);
  const phase = useAppUpdateStore((s) => s.phase);
  const progress = useAppUpdateStore((s) => s.progress);
  const openModal = useAppUpdateStore((s) => s.openModal);

  if (!hasUpdate || dismissed) return null;

  return (
    <button
      type="button"
      onClick={openModal}
      className="flex items-center gap-1.5 rounded-lg border border-cyan-500/50 bg-gradient-to-r from-cyan-950/60 to-blue-950/60 px-2.5 py-1.5 text-xs font-medium text-cyan-300 shadow-[0_0_12px_rgba(6,182,212,0.15)] hover:border-cyan-400 hover:from-cyan-900/80 hover:to-blue-900/80 active:scale-[0.98] transition-all"
      title="Click to view release notes and install"
    >
      {phase === "downloading" ? (
        <>
          <ArrowDownCircle className="h-3.5 w-3.5 animate-bounce text-cyan-400" />
          <span>
            {progress !== null ? `Downloading ${Math.round(progress * 100)}%` : "Downloading..."}
          </span>
        </>
      ) : phase === "ready" ? (
        <>
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
            <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500"></span>
          </span>
          <span className="text-emerald-300">Ready to Restart</span>
        </>
      ) : (
        <>
          <Sparkles className="h-3.5 w-3.5 text-cyan-400 animate-pulse" />
          <span>v{availableVersion} available</span>
        </>
      )}
    </button>
  );
}
