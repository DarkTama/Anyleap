import { Smartphone, TabletSmartphone, Monitor, Sliders, Wifi, Command } from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { UpdateBadge } from "./UpdateBadge";
import { AppLogo } from "./AppLogo";

export type ShellTab = "devices" | "phone" | "computers" | "settings";

interface AppHeaderProps {
  activeTab: ShellTab;
  onTabChange: (tab: ShellTab) => void;
  onOpenPair: () => void;
  onOpenCommands: () => void;
}

export function AppHeader({ activeTab, onTabChange, onOpenPair, onOpenCommands }: AppHeaderProps) {
  const sessions = useAppStore((s) => s.sessions);
  const currentVersion = useAppUpdateStore((s) => s.currentVersion);

  return (
    <header className="flex h-13 shrink-0 items-center justify-between border-b border-ink-700 bg-ink-950 px-4 select-none">
      {/* Brand & Version Chip */}
      <div className="flex items-center gap-2.5">
        <div className="flex items-center gap-2 font-bold tracking-tight text-text-strong text-[13px]">
          <AppLogo className="h-5 w-5 shrink-0" />
          <span className="text-text-strong">AnyLeap</span>
          <span className="rounded border border-ink-700 bg-ink-900 px-1.5 py-0.5 font-mono text-[10px] font-medium text-text-muted">
            v{currentVersion}
          </span>
        </div>
      </div>

      {/* Segmented Navigation Tabs */}
      <nav className="flex items-center rounded-lg border border-ink-700 bg-ink-900 p-0.5">
        <button
          type="button"
          onClick={() => onTabChange("devices")}
          className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium cursor-pointer ${
            activeTab === "devices"
              ? "bg-ink-800 text-gold-400"
              : "text-text-muted hover:text-text-strong"
          }`}
        >
          <Smartphone className="h-3.5 w-3.5" />
          <span>Devices</span>
        </button>

        <button
          type="button"
          onClick={() => onTabChange("phone")}
          className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium cursor-pointer ${
            activeTab === "phone"
              ? "bg-ink-800 text-gold-400"
              : "text-text-muted hover:text-text-strong"
          }`}
        >
          <TabletSmartphone className="h-3.5 w-3.5" />
          <span>Phone</span>
        </button>

        <button
          type="button"
          onClick={() => onTabChange("computers")}
          className={`btn-tactile flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium cursor-pointer ${
            activeTab === "computers"
              ? "bg-ink-800 text-gold-400"
              : "text-text-muted hover:text-text-strong"
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
              ? "bg-ink-800 text-gold-400"
              : "text-text-muted hover:text-text-strong"
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
          onClick={onOpenCommands}
          className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-900 px-2.5 py-1 text-xs font-medium text-text hover:border-text-muted hover:bg-ink-800 cursor-pointer"
          title="Open command palette (Ctrl+K)"
        >
          <Command className="h-3.5 w-3.5 text-text-muted" />
          <span className="hidden sm:inline">Commands</span>
          <kbd className="hidden md:inline rounded border border-ink-700 bg-ink-950 px-1 py-0.2 font-mono text-[10px] text-text-muted">
            Ctrl+K
          </kbd>
        </button>

        <button
          type="button"
          onClick={onOpenPair}
          className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-900 px-2.5 py-1 text-xs font-medium text-text hover:border-text-muted hover:bg-ink-800 cursor-pointer"
        >
          <Wifi className="h-3.5 w-3.5 text-text-muted" />
          <span className="hidden sm:inline">Pair Device</span>
        </button>

        <div className="flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-900 px-2.5 py-1 font-mono text-[11px]">
          <span
            className={`h-2 w-2 rounded-full ${
              sessions.length > 0 ? "bg-status-success" : "bg-text-muted"
            }`}
          />
          <span className={sessions.length > 0 ? "text-status-success font-semibold" : "text-text-muted"}>
            {sessions.length > 0 ? `${sessions.length} LIVE` : "STANDBY"}
          </span>
        </div>
      </div>
    </header>
  );
}
