import { useEffect, useId, useState } from "react";
import { FolderOpen, RotateCcw } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useKvmStore } from "@/stores/kvmStore";
import { useAppStore } from "@/store/useAppStore";
import { saveTransferFolders } from "@/lib/persist";
import { setTransferFoldersConfig } from "@/lib/tauri";
import {
  DEFAULT_PHONE_UPLOAD_DIR,
  PHONE_UPLOAD_PRESETS,
  normalizePhoneUploadDir,
  resolveDisplayReceiveDir,
  validatePhoneUploadDir,
  type TransferFoldersSettings,
} from "@/lib/transferFolders";
import { selectClass } from "@/lib/styles";
import { entry } from "./registry";
import { Panel } from "./Panel";
import { SwitchRow } from "./SwitchRow";
import type { ShowFn } from "./SettingsShell";

/** Clipboard & files (K-06, K-07, T-01..T-03, F-01..F-03). */
export function ClipboardFilesSection({ show }: { show: ShowFn }) {
  const kvm = useKvmStore((s) => s.settings);
  const update = useKvmStore((s) => s.updateSettings);

  const transferFolders = useAppStore((s) => s.transferFolders);
  const defaultTransferFolders = useAppStore((s) => s.defaultTransferFolders);
  const setTransferFolders = useAppStore((s) => s.setTransferFolders);

  const [phoneDraft, setPhoneDraft] = useState(transferFolders.phoneUploadDir);
  const [phoneError, setPhoneError] = useState<string | null>(null);

  useEffect(() => {
    setPhoneDraft(transferFolders.phoneUploadDir);
    setPhoneError(null);
  }, [transferFolders.phoneUploadDir]);

  const f01Id = useId();
  const f03Id = useId();

  const persistFolders = (next: TransferFoldersSettings) => {
    setTransferFolders(next);
    void saveTransferFolders(next).catch(() => {});
    void setTransferFoldersConfig(next.receiveDir, next.phoneUploadDir).catch(() => {});
  };

  const displayReceiveDir = resolveDisplayReceiveDir(
    transferFolders.receiveDir,
    defaultTransferFolders.defaultReceiveDir,
  );

  const handleChangeReceiveDir = async () => {
    const picked = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: displayReceiveDir,
    }).catch(() => null);
    if (!picked || typeof picked !== "string") return;
    persistFolders({ ...transferFolders, receiveDir: picked });
  };

  const handlePhoneInputChange = (raw: string) => {
    setPhoneDraft(raw);
    const err = validatePhoneUploadDir(raw);
    if (err) {
      setPhoneError(err);
      return;
    }
    setPhoneError(null);
    const norm = normalizePhoneUploadDir(raw);
    if (norm.ok) {
      persistFolders({ ...transferFolders, phoneUploadDir: norm.path });
    }
  };

  const handlePresetPhoneDir = (path: string) => {
    setPhoneDraft(path);
    setPhoneError(null);
    persistFolders({ ...transferFolders, phoneUploadDir: path });
  };

  const showTop =
    show("K-06") || show("K-07") || show("T-01") || show("T-02") || show("T-03");
  const showFolders = show("F-01") || show("F-02") || show("F-03");

  return (
    <Panel className="space-y-3">
      {showTop && (
        <div className="space-y-1">
          {show("K-06") && (
            <SwitchRow
              label={entry("K-06").label}
              hint={entry("K-06").helper}
              checked={kvm.syncClipboardText}
              onChange={(v) => update({ syncClipboardText: v })}
            />
          )}
          {show("K-07") && (
            <SwitchRow
              label={entry("K-07").label}
              hint={entry("K-07").helper}
              checked={kvm.syncClipboardImage}
              onChange={(v) => update({ syncClipboardImage: v })}
            />
          )}
          {show("T-01") && (
            <SwitchRow
              label={entry("T-01").label}
              hint={entry("T-01").helper}
              checked={kvm.continuityDragDropExperimental}
              onChange={(v) => update({ continuityDragDropExperimental: v })}
            />
          )}
          {show("T-02") && (
            <SwitchRow
              label={entry("T-02").label}
              hint={entry("T-02").helper}
              checked={kvm.continuityFileClipboard}
              onChange={(v) => update({ continuityFileClipboard: v })}
            />
          )}
          {show("T-03") && (
            <SwitchRow
              label={entry("T-03").label}
              checked={kvm.continuityPointerScheme}
              onChange={(v) => update({ continuityPointerScheme: v })}
            />
          )}
        </div>
      )}

      {showFolders && (
        <div className={showTop ? "space-y-3 border-t border-ink-700 pt-3" : "space-y-3"}>
          <h3 className="text-xs font-semibold uppercase tracking-wider text-text-muted">
            Transfer folders
          </h3>

          {show("F-01") && (
            <div className="space-y-1.5 py-1">
              <div>
                <div id={`${f01Id}-label`} className="text-sm text-text-strong">
                  {entry("F-01").label}
                </div>
                <p id={`${f01Id}-hint`} className="text-[11px] leading-snug text-text-muted">
                  {entry("F-01").helper}
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <output
                  aria-labelledby={`${f01Id}-label`}
                  aria-describedby={`${f01Id}-hint`}
                  className="min-w-0 flex-1 truncate rounded-lg border border-ink-700 bg-ink-950 px-3 py-1.5 font-mono text-[11px] text-text"
                >
                  {displayReceiveDir}
                </output>
                <button
                  type="button"
                  onClick={() => void handleChangeReceiveDir()}
                  className="btn-tactile min-h-8 shrink-0 cursor-pointer rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text-strong hover:border-text-muted focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                >
                  Change...
                </button>
                <button
                  type="button"
                  onClick={() => void revealItemInDir(displayReceiveDir).catch(() => {})}
                  className="btn-tactile flex min-h-8 shrink-0 cursor-pointer items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                >
                  <FolderOpen aria-hidden="true" className="h-3.5 w-3.5" />
                  <span>Open</span>
                </button>
                {transferFolders.receiveDir !== null && (
                  <button
                    type="button"
                    onClick={() => persistFolders({ ...transferFolders, receiveDir: null })}
                    className="btn-tactile flex min-h-8 shrink-0 cursor-pointer items-center gap-1 rounded-lg border border-ink-700 bg-ink-800 px-2.5 py-1.5 text-xs text-text-muted hover:border-text-muted hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                  >
                    <RotateCcw aria-hidden="true" className="h-3 w-3" />
                    <span>Reset</span>
                  </button>
                )}
              </div>
            </div>
          )}

          {show("F-02") && (
            <SwitchRow
              label={entry("F-02").label}
              hint={entry("F-02").helper}
              checked={transferFolders.askWhereToSave}
              onChange={(v) => persistFolders({ ...transferFolders, askWhereToSave: v })}
            />
          )}

          {show("F-03") && (
            <div className="space-y-1.5 py-1">
              <label htmlFor={f03Id} className="text-sm text-text-strong">
                {entry("F-03").label}
              </label>
              <div className="flex flex-wrap items-center gap-2">
                <input
                  id={f03Id}
                  type="text"
                  value={phoneDraft}
                  aria-describedby={phoneError ? `${f03Id}-error ${f03Id}-hint` : `${f03Id}-hint`}
                  aria-invalid={phoneError ? true : undefined}
                  onChange={(e) => handlePhoneInputChange(e.target.value)}
                  className={`${selectClass} min-w-0 flex-1 font-mono text-xs`}
                />
                {(transferFolders.phoneUploadDir !== DEFAULT_PHONE_UPLOAD_DIR ||
                  phoneDraft !== DEFAULT_PHONE_UPLOAD_DIR) && (
                  <button
                    type="button"
                    onClick={() => handlePresetPhoneDir(DEFAULT_PHONE_UPLOAD_DIR)}
                    className="btn-tactile flex min-h-8 shrink-0 cursor-pointer items-center gap-1 rounded-lg border border-ink-700 bg-ink-800 px-2.5 py-1.5 text-xs text-text-muted hover:border-text-muted hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
                  >
                    <RotateCcw aria-hidden="true" className="h-3 w-3" />
                    <span>Reset</span>
                  </button>
                )}
              </div>
              <div className="flex flex-wrap items-center gap-1.5">
                {PHONE_UPLOAD_PRESETS.map((preset) => {
                  const active = transferFolders.phoneUploadDir === preset.path && !phoneError;
                  return (
                    <button
                      key={preset.path}
                      type="button"
                      aria-pressed={active}
                      onClick={() => handlePresetPhoneDir(preset.path)}
                      className={`btn-tactile cursor-pointer rounded-md border px-2 py-0.5 text-[11px] font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 ${
                        active
                          ? "border-gold-400 bg-ink-800 text-gold-400"
                          : "border-ink-700 bg-ink-950 text-text-muted hover:border-text-muted hover:text-text-strong"
                      }`}
                    >
                      {preset.label}
                    </button>
                  );
                })}
              </div>
              {phoneError && (
                <p id={`${f03Id}-error`} role="alert" className="text-[11px] text-status-danger">
                  {phoneError}
                </p>
              )}
              <p id={`${f03Id}-hint`} className="text-[11px] leading-snug text-text-muted">
                {entry("F-03").helper}
              </p>
            </div>
          )}
        </div>
      )}
    </Panel>
  );
}
