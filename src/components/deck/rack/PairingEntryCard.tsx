import { Plus, Wifi } from "lucide-react";

interface PairingEntryCardProps {
  onOpenPair: () => void;
}

/** Dashed pairing callout at the bottom of the device rack. */
export function PairingEntryCard({ onOpenPair }: PairingEntryCardProps) {
  return (
    <div className="rounded-xl border border-dashed border-ink-700 p-3 space-y-2">
      <div className="flex items-center gap-1.5 text-xs font-semibold text-text-strong">
        <Plus className="h-3.5 w-3.5 text-text" />
        <span>Add a phone</span>
      </div>
      <p className="text-[11px] leading-relaxed text-text-muted">
        Pair over Wi-Fi with a code, or plug in via USB with debugging enabled.
      </p>
      <button
        type="button"
        onClick={onOpenPair}
        className="btn-tactile w-full flex items-center justify-center gap-1.5 rounded-lg bg-gold-400 px-3 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer"
      >
        <Wifi className="h-3.5 w-3.5" />
        <span>Pair Device</span>
      </button>
    </div>
  );
}
