import { Smartphone, Monitor, Sliders, Wifi } from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { UpdateBadge } from "./UpdateBadge";
import { AppLogo } from "./AppLogo";

export type ShellTab = "devices" | "computers" | "settings";

interface AppHeaderProps {
  activeTab: ShellTab;
  onTabChange: (tab: ShellTab) => void;
  onOpenPair: () => void;
}

export function AppHeader({ activeTab, onTabChange, onOpenPair }: AppHeaderProps) {
  const sessions = useAppStore((s) => s.sessions);
  const currentVersion = useAppUpdateStore((s) => s.currentVersion);

  return (
    <header className="flex h-13 shrink-0 items-center justify-between border-b border-[#222226] bg-[#0c0c0e] px-4 select-none">
      {/* Brand & Version Chip */}
      <div className="flex items-center gap-2.5">
        <div className="flex items-center gap-2 font-bold tracking-tight text-zinc-100 text-[13px]">
          <AppLogo className="h-5 w-5 shrink-0" />
          <span className="bg-gradient-to-r from-zinc-100 to-zinc-400 bg-clip-text text-transparent">
            AnyLeap
          </span>
          <span className="rounded bg-zinc-800/80 px-1.5 py-0.5 font-mono text-[10px] font-medium text-zinc-400">
            v{currentVersion}
          </span>
        </div>
      </div>

      {/* Segmented Navigation Tabs */}
      <nav className="flex items-center rounded-lg border border-[#222226] bg-[#141417] p-0.5">
        <button
          type="button"
          onClick={() => onTabChange("devices")}
          className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium cursor-pointer ${
            activeTab === "devices"
              ? "bg-[#222228] text-zinc-100 shadow-sm"
              : "text-zinc-400 hover:text-zinc-200"
          }`}
        >
          <Smartphone className="h-3.5 w-3.5" />
          <span>Devices</span>
        </button>

        <button
          type="button"
          onClick={() => onTabChange("computers")}
          className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium cursor-pointer ${
            activeTab === "computers"
              ? "bg-[#222228] text-zinc-100 shadow-sm"
              : "text-zinc-400 hover:text-zinc-200"
          }`}
        >
          <Monitor className="h-3.5 w-3.5" />
          <span>Computers</span>
        </button>

        <button
          type="button"
          onClick={() => onTabChange("settings")}
          className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium cursor-pointer ${
            activeTab === "settings"
              ? "bg-[#222228] text-zinc-100 shadow-sm"
              : "text-zinc-400 hover:text-zinc-200"
          }`}
        >
          <Sliders className="h-3.5 w-3.5" />
          <span>Settings</span>
        </button>
      </nav>

      {/* Status, Update Badge & Actions */}
      <div className="flex items-center gap-2.5">
        <UpdateBadge />

        <button
          type="button"
          onClick={onOpenPair}
          className="btn-tactile flex items-center gap-1.5 rounded-lg border border-[#27272a] bg-[#16161a] px-2.5 py-1 text-xs font-medium text-zinc-300 hover:border-zinc-600 hover:bg-[#1c1c21] cursor-pointer"
        >
          <Wifi className="h-3.5 w-3.5 text-zinc-400" />
          <span className="hidden sm:inline">Pair Device</span>
        </button>

        <div className="flex items-center gap-1.5 rounded-md border border-[#222226] bg-[#121215] px-2.5 py-1 font-mono text-[11px]">
          <span
            className={`h-2 w-2 rounded-full ${
              sessions.length > 0 ? "bg-emerald-400 animate-pulse" : "bg-zinc-600"
            }`}
          />
          <span className={sessions.length > 0 ? "text-emerald-300 font-semibold" : "text-zinc-400"}>
            {sessions.length > 0 ? `${sessions.length} LIVE` : "STANDBY"}
          </span>
        </div>
      </div>
    </header>
  );
}
