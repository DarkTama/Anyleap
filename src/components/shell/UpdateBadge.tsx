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
      className="btn-tactile flex items-center gap-1.5 rounded-lg border border-gold-400/60 bg-ink-800 px-2.5 py-1.5 text-xs font-medium text-gold-400 hover:border-gold-400 hover:bg-ink-700 cursor-pointer"
      title="Click to view release notes and install"
    >
      {phase === "downloading" ? (
        <>
          <ArrowDownCircle className="h-3.5 w-3.5 text-gold-400" />
          <span>
            {progress !== null ? `Downloading ${Math.round(progress * 100)}%` : "Downloading..."}
          </span>
        </>
      ) : phase === "ready" ? (
        <>
          <span className="inline-flex h-2 w-2 rounded-full bg-status-success" />
          <span className="text-status-success">Ready to Restart</span>
        </>
      ) : (
        <>
          <Sparkles className="h-3.5 w-3.5 text-gold-400" />
          <span>v{availableVersion} available</span>
        </>
      )}
    </button>
  );
}
