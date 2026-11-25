import { useState } from "react";
import { ExternalLink, FolderOpen, ShieldCheck, Trash2 } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { kvmDeleteDebugLogs, openAppDataFolder } from "@/services/kvmService";
import { entry } from "./registry";
import { Panel } from "./Panel";
import type { ShowFn } from "./SettingsShell";

export const PRIVACY_POLICY_URL =
  "https://github.com/DarkTama/Anyleap/blob/main/PRIVACY.md";

/** About & Privacy (A-01): version, local-first privacy summary, policy link, data folder, and log cleanup. */
export function AboutSection({ show }: { show: ShowFn }) {
  const currentVersion = useAppUpdateStore((s) => s.currentVersion);
  const [deleteStatus, setDeleteStatus] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  const handleDeleteLogs = async () => {
    setDeleting(true);
    try {
      const removed = await kvmDeleteDebugLogs();
      setDeleteStatus(
        removed > 0
          ? `Deleted ${removed} log ${removed === 1 ? "file" : "files"}.`
          : "No log files to delete.",
      );
    } catch {
      setDeleteStatus("Could not delete log files.");
    } finally {
      setDeleting(false);
    }
  };

  return (
    <Panel className="space-y-4">
      <div className="flex items-center justify-between border-b border-ink-700 pb-3">
        <div>
          <div className="text-sm font-medium text-text-strong">AnyLeap</div>
          <p className="text-[11px] text-text-muted">Version v{currentVersion}</p>
        </div>
      </div>

      {show("A-01") && (
        <div className="space-y-3">
          <div className="flex items-start gap-2.5">
            <ShieldCheck aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-gold-400" />
            <div className="space-y-1">
              <h3 className="text-sm font-medium text-text-strong">{entry("A-01").label}</h3>
              <p className="text-xs leading-relaxed text-text-muted">{entry("A-01").helper}</p>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2 pt-1">
            <a
              href={PRIVACY_POLICY_URL}
              target="_blank"
              rel="noreferrer"
              onClick={(e) => {
                e.preventDefault();
                void openUrl(PRIVACY_POLICY_URL).catch(() => {});
              }}
              className="btn-tactile flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-gold-400/50 bg-ink-800 px-3 py-1.5 text-xs font-medium text-gold-400 hover:bg-ink-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
            >
              <ExternalLink aria-hidden="true" className="h-3.5 w-3.5" />
              <span>Open privacy policy</span>
            </a>

            <button
              type="button"
              onClick={() => void openAppDataFolder().catch(() => {})}
              className="btn-tactile flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
            >
              <FolderOpen aria-hidden="true" className="h-3.5 w-3.5" />
              <span>Open data folder</span>
            </button>

            <button
              type="button"
              disabled={deleting}
              onClick={() => void handleDeleteLogs()}
              className="btn-tactile flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-status-danger/60 hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 disabled:opacity-50"
            >
              <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
              <span>Delete logs</span>
            </button>
          </div>

          {deleteStatus && (
            <p aria-live="polite" className="text-[11px] text-text-muted">
              {deleteStatus}
            </p>
          )}
        </div>
      )}
    </Panel>
  );
}
