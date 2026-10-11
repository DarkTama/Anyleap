import { MousePointer, Laptop, Monitor } from "lucide-react";
import { useKvmStore } from "@/stores/kvmStore";

export function DesktopHudPill() {
  const status = useKvmStore((s) => s.status);
  const settings = useKvmStore((s) => s.settings);
  const topology = useKvmStore((s) => s.topology);

  if (!settings.indicatorHud) return null;

  const peer = topology.computers.find((c) => !c.is_local);
  if (!peer && !status.is_host_running) return null;

  const isRemote = status.is_input_trapped;
  const activeName = isRemote
    ? peer?.name || "Remote PC"
    : status.local_name || "This PC";

  // Corner positioning
  const cornerClasses = {
    "top-left": "top-4 left-4",
    "top-right": "top-4 right-4",
    "bottom-left": "bottom-4 left-4",
    "bottom-right": "bottom-4 right-4",
  }[settings.indicatorHudCorner || "top-right"];

  return (
    <div
      className={`fixed ${cornerClasses} z-40 pointer-events-none transition-all duration-300 ease-out`}
    >
      <div
        className={`flex items-center gap-2 rounded-full border px-3 py-1.5 shadow-2xl backdrop-blur-md text-xs font-mono transition-all ${
          isRemote
            ? "border-sky-400 bg-sky-950/80 text-sky-200 shadow-[0_0_20px_rgba(56,189,248,0.35)]"
            : "border-zinc-700 bg-zinc-900/80 text-zinc-300 shadow-lg"
        }`}
      >
        <span
          className={`h-2 w-2 rounded-full ${
            isRemote ? "bg-sky-400 animate-ping" : "bg-emerald-400"
          }`}
        />
        {isRemote ? (
          <Laptop className="h-3.5 w-3.5 text-sky-400" />
        ) : (
          <Monitor className="h-3.5 w-3.5 text-zinc-400" />
        )}
        <span className="font-semibold tracking-wide truncate max-w-[150px]">
          {activeName}
        </span>
        <span className="text-[10px] uppercase font-bold text-zinc-400 border-l border-zinc-700 pl-2">
          {isRemote ? "REMOTE" : "LOCAL"}
        </span>
        {isRemote && (
          <MousePointer className="h-3 w-3 text-sky-300 animate-pulse" />
        )}
      </div>
    </div>
  );
}
