import { RefreshCw, Sparkles } from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { saveAppPrefs, type AppPrefs } from "@/lib/persist";
import { entry } from "./registry";
import { Panel } from "./Panel";
import { SwitchRow } from "./SwitchRow";
import type { ShowFn } from "./SettingsShell";

/** General (M-25..M-29): tray, updates, notifications, battery alerts. */
export function GeneralSection({ show }: { show: ShowFn }) {
  const appPrefs = useAppStore((s) => s.appPrefs);
  const setAppPrefs = useAppStore((s) => s.setAppPrefs);
  const update = (next: AppPrefs) => {
    setAppPrefs(next);
    void saveAppPrefs(next);
  };

  const updatePhase = useAppUpdateStore((s) => s.phase);
  const hasUpdate = useAppUpdateStore((s) => s.hasUpdate);
  const availableVersion = useAppUpdateStore((s) => s.availableVersion);
  const currentVersion = useAppUpdateStore((s) => s.currentVersion);
  const openUpdateModal = useAppUpdateStore((s) => s.openModal);
  const checkForUpdatesNow = useAppUpdateStore((s) => s.checkForUpdates);

  const row = (id: string, key: keyof AppPrefs) =>
    show(id) && (
      <SwitchRow
        label={entry(id).label}
        hint={entry(id).helper}
        checked={Boolean(appPrefs[key])}
        onChange={(v) => update({ ...appPrefs, [key]: v })}
      />
    );

  return (
    <Panel>
      {row("M-25", "minimizeToTrayOnClose")}
      {row("M-26", "checkUpdates")}
      {row("M-27", "desktopNotifications")}
      {row("M-28", "batteryAlerts")}
      {show("M-29") && (
        <div className="flex items-center justify-between gap-3 pt-2">
          <div>
            <div className="text-sm text-text-strong">{entry("M-29").label}</div>
            <p className="text-[11px] text-text-muted">
              {updatePhase === "checking"
                ? "Checking for updates..."
                : hasUpdate
                  ? `v${availableVersion} available`
                  : `Current version v${currentVersion}`}
            </p>
          </div>
          {hasUpdate ? (
            <button
              type="button"
              onClick={openUpdateModal}
              className="btn-tactile flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-gold-400/50 bg-ink-800 px-3 py-1.5 text-xs font-medium text-gold-400 hover:bg-ink-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
            >
              <Sparkles aria-hidden="true" className="h-3 w-3 text-gold-400" />
              <span>View update</span>
            </button>
          ) : (
            <button
              type="button"
              disabled={updatePhase === "checking"}
              onClick={() => void checkForUpdatesNow(true)}
              className="btn-tactile flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 disabled:opacity-50"
            >
              <RefreshCw
                aria-hidden="true"
                className={`h-3 w-3 ${
                  updatePhase === "checking" ? "animate-spin motion-reduce:animate-none" : ""
                }`}
              />
              <span>Check now</span>
            </button>
          )}
        </div>
      )}
    </Panel>
  );
}
