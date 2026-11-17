import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ArrowLeftRight, FolderOpen, RefreshCw, Sparkles, Laptop } from "lucide-react";
import { PhoneMirroringSection } from "@/components/settings/PhoneMirroringSection";
import { useAppStore } from "@/store/useAppStore";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { optionClass, selectClass } from "@/lib/styles";
import { emit } from "@tauri-apps/api/event";
import {
  BUTTON_IDS,
  BUTTON_LABELS,
  CONTROL_SIZES,
  DOCK_SIDES,
  saveControlConfig,
  type ControlConfig,
} from "@/lib/controlConfig";
import { saveAppPrefs, type AppPrefs } from "@/lib/persist";
import { useKvmStore } from "@/stores/kvmStore";
import {
  kvmGetDebugLogPath,
  kvmOpenDebugLogFolder,
  kvmSetSyncSettings,
} from "@/services/kvmService";

const SIZE_LABELS: Record<ControlConfig["size"], string> = {
  sm: "Small",
  md: "Medium",
  lg: "Large",
};

export function SettingsPanel() {
  const controlConfig = useAppStore((s) => s.controlConfig);
  const setControlConfig = useAppStore((s) => s.setControlConfig);

  function updateControl(next: ControlConfig) {
    setControlConfig(next);
    void saveControlConfig(next);
    void emit("control-config", next); // live-update the floating control window
  }

  const appPrefs = useAppStore((s) => s.appPrefs);
  const setAppPrefs = useAppStore((s) => s.setAppPrefs);
  function updateAppPrefs(next: AppPrefs) {
    setAppPrefs(next);
    void saveAppPrefs(next);
  }

  const updatePhase = useAppUpdateStore((s) => s.phase);
  const hasUpdate = useAppUpdateStore((s) => s.hasUpdate);
  const availableVersion = useAppUpdateStore((s) => s.availableVersion);
  const currentVersion = useAppUpdateStore((s) => s.currentVersion);
  const openUpdateModal = useAppUpdateStore((s) => s.openModal);
  const checkForUpdatesNow = useAppUpdateStore((s) => s.checkForUpdates);

  return (
    <div className="space-y-4">
    <PhoneMirroringSection />

    <Card>
      <CardHeader>
        <CardTitle>General</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="space-y-2">
          <Toggle
            label="Minimize to tray when closing the window"
            checked={appPrefs.minimizeToTrayOnClose}
            onChange={(v) => updateAppPrefs({ ...appPrefs, minimizeToTrayOnClose: v })}
          />
          <Toggle
            label="Check for updates on launch"
            checked={appPrefs.checkUpdates}
            onChange={(v) => updateAppPrefs({ ...appPrefs, checkUpdates: v })}
          />
          <Toggle
            label="Show phone notifications on this PC"
            checked={appPrefs.desktopNotifications}
            onChange={(v) => updateAppPrefs({ ...appPrefs, desktopNotifications: v })}
          />
          <Toggle
            label="Battery alerts (low ≤20% & full 100%)"
            checked={appPrefs.batteryAlerts}
            onChange={(v) => updateAppPrefs({ ...appPrefs, batteryAlerts: v })}
          />

          <div className="flex items-center justify-between pt-2">
            <div>
              <div className="text-xs font-medium text-text">Application Updates</div>
              <div className="text-[11px] text-text-muted">
                {updatePhase === "checking"
                  ? "Checking GitHub releases..."
                  : hasUpdate
                  ? `v${availableVersion} available`
                  : `Current version: v${currentVersion}`}
              </div>
            </div>

            {hasUpdate ? (
              <button
                type="button"
                onClick={openUpdateModal}
                className="btn-tactile flex items-center gap-1.5 rounded-lg border border-gold-400/50 bg-ink-800 px-3 py-1.5 text-xs font-medium text-gold-400 hover:bg-ink-700 cursor-pointer"
              >
                <Sparkles className="h-3 w-3 text-gold-400" />
                <span>View Update</span>
              </button>
            ) : (
              <button
                type="button"
                disabled={updatePhase === "checking"}
                onClick={() => void checkForUpdatesNow(true)}
                className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong cursor-pointer disabled:opacity-50"
              >
                <RefreshCw
                  className={`h-3 w-3 ${
                    updatePhase === "checking" ? "animate-spin text-text-strong" : "text-text-muted"
                  }`}
                />
                <span>Check Now</span>
              </button>
            )}
          </div>
        </div>
      </CardContent>
    </Card>

    <Card>
      <CardHeader>
        <CardTitle>Control bar</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid grid-cols-2 gap-3">
          <div className="space-y-1">
            <label className="text-xs font-medium text-text-muted">Dock position</label>
            <select
              className={selectClass}
              value={controlConfig.dock}
              onChange={(e) =>
                updateControl({ ...controlConfig, dock: e.target.value as ControlConfig["dock"] })
              }
            >
              {DOCK_SIDES.map((d) => (
                <option key={d} value={d} className={optionClass}>
                  {d === "undocked" ? "Undocked (free)" : d[0].toUpperCase() + d.slice(1)}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-1">
            <label className="text-xs font-medium text-text-muted">Size</label>
            <select
              className={selectClass}
              value={controlConfig.size}
              onChange={(e) =>
                updateControl({ ...controlConfig, size: e.target.value as ControlConfig["size"] })
              }
            >
              {CONTROL_SIZES.map((s) => (
                <option key={s} value={s} className={optionClass}>
                  {SIZE_LABELS[s]}
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="space-y-1">
          <label className="text-xs font-medium text-text-muted">Buttons</label>
          <div className="grid grid-cols-2 gap-2">
            {BUTTON_IDS.map((id) => (
              <label key={id} className="flex items-center gap-2 text-sm text-text">
                <input
                  type="checkbox"
                  className="h-4 w-4 accent-gold-400"
                  checked={controlConfig.buttons[id]}
                  onChange={(e) =>
                    updateControl({
                      ...controlConfig,
                      buttons: { ...controlConfig.buttons, [id]: e.target.checked },
                    })
                  }
                />
                {BUTTON_LABELS[id]}
              </label>
            ))}
          </div>
        </div>

        <p className="text-[11px] text-text-muted">
          Applies to the floating control strip and the in-app controls; changes take effect
          immediately.
        </p>
      </CardContent>
    </Card>

    <KvmSettingsCard />
    <ContinuitySettingsCard />
    </div>
  );
}

function KvmSettingsCard() {
  const kvmSettings = useKvmStore((s) => s.settings);
  const updateKvmSettings = useKvmStore((s) => s.updateSettings);
  const [debugLogPath, setDebugLogPath] = useState<string>("");

  useEffect(() => {
    void kvmSetSyncSettings(
      kvmSettings.syncClipboardText,
      kvmSettings.syncClipboardImage,
      // Retired gate (SPEC-28 §9.3): always on; "Copy-paste files" is the only file switch.
      true
    ).catch(() => {});
  }, [kvmSettings.syncClipboardText, kvmSettings.syncClipboardImage]);

  useEffect(() => {
    void kvmGetDebugLogPath()
      .then((path) => setDebugLogPath(path))
      .catch(() => {});
  }, []);

  const handleDebugLogToggle = (enabled: boolean) => {
    updateKvmSettings({ debugLog: enabled });
    if (!debugLogPath) {
      void kvmGetDebugLogPath()
        .then((path) => setDebugLogPath(path))
        .catch(() => {});
    }
  };

  return (
    <Card className="rounded-xl border border-ink-700 bg-ink-900">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-sm font-semibold text-text-strong">
          <Laptop className="h-4 w-4 text-mist-300" />
          Shared Mouse &amp; Keyboard
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-5">
        {/* Edge Resistance Slider */}
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-xs">
            <label className="font-medium text-text">
              Edge Push Resistance (Dwell Delay)
            </label>
            <span className="font-mono text-gold-400 font-semibold">
              {kvmSettings.edgeResistanceMs} ms
            </span>
          </div>
          <input
            type="range"
            min={0}
            max={300}
            step={10}
            value={kvmSettings.edgeResistanceMs}
            onChange={(e) =>
              updateKvmSettings({ edgeResistanceMs: Number(e.target.value) })
            }
            className="w-full accent-gold-400 cursor-pointer"
          />
          <p className="text-[11px] text-text-muted">
            Prevents accidental leaps when interacting with browser sidebars or scrollbars. Set to 0ms for instant leap.
          </p>
        </div>

        {/* Corner Deadzone Slider */}
        <div className="space-y-1.5">
          <div className="flex items-center justify-between text-xs">
            <label className="font-medium text-text">
              Corner Deadzone Margin
            </label>
            <span className="font-mono text-status-warning font-semibold">
              {kvmSettings.cornerDeadzonePx} px
            </span>
          </div>
          <input
            type="range"
            min={0}
            max={120}
            step={4}
            value={kvmSettings.cornerDeadzonePx}
            onChange={(e) =>
              updateKvmSettings({ cornerDeadzonePx: Number(e.target.value) })
            }
            className="w-full accent-gold-400 cursor-pointer"
          />
          <p className="text-[11px] text-text-muted">
            Top and bottom corner screen zones where leaps are blocked (tabs, titlebars, taskbars).
          </p>
        </div>

        {/* Indicators */}
        <div className="space-y-2.5 pt-2 border-t border-ink-700">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
            Visual Indicators
          </span>
          <div className="grid grid-cols-2 gap-3">
            <Toggle
              label="Floating Desktop HUD Pill"
              checked={kvmSettings.indicatorHud}
              onChange={(checked) =>
                updateKvmSettings({ indicatorHud: checked })
              }
            />
            <Toggle
              label="Canvas Traveling Cursor Beacon"
              checked={kvmSettings.indicatorCanvasDot}
              onChange={(checked) =>
                updateKvmSettings({ indicatorCanvasDot: checked })
              }
            />
          </div>

          {kvmSettings.indicatorHud && (
            <div className="space-y-1 pt-1 max-w-xs">
              <label className="text-xs font-medium text-text-muted">
                Desktop HUD Corner
              </label>
              <select
                className={selectClass}
                value={kvmSettings.indicatorHudCorner}
                onChange={(e) =>
                  updateKvmSettings({
                    indicatorHudCorner: e.target.value as any,
                  })
                }
              >
                <option value="top-right" className={optionClass}>
                  Top Right
                </option>
                <option value="top-left" className={optionClass}>
                  Top Left
                </option>
                <option value="bottom-right" className={optionClass}>
                  Bottom Right
                </option>
                <option value="bottom-left" className={optionClass}>
                  Bottom Left
                </option>
              </select>
            </div>
          )}
        </div>

        {/* Emergency Escape Shortcuts (Fixed) */}
        <div className="space-y-2 pt-2 border-t border-ink-700">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
            Emergency Unhook Shortcut
          </span>
          <div className="space-y-1">
            <div className="rounded-lg border border-ink-700 bg-ink-950 px-3 py-2 text-xs text-text-strong font-mono">
              Scroll Lock, Pause, or Ctrl+Alt+Esc
            </div>
            <p className="text-[11px] text-text-muted">
              Immediately breaks cursor trap, unclips boundary lock, and returns pointer to host.
            </p>
          </div>
        </div>

        {/* Media & Clipboard Synchronization */}
        <div className="space-y-2.5 pt-2 border-t border-ink-700">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
            Clipboard & Media Sync
          </span>
          <div className="space-y-2">
            <Toggle
              label="Sync Text Clipboard"
              checked={kvmSettings.syncClipboardText}
              onChange={(checked) =>
                updateKvmSettings({ syncClipboardText: checked })
              }
            />
            <Toggle
              label="Sync Image Clipboard (Screenshots & Bitmaps)"
              checked={kvmSettings.syncClipboardImage}
              onChange={(checked) =>
                updateKvmSettings({ syncClipboardImage: checked })
              }
            />
          </div>
        </div>

        {/* Diagnostics */}
        <div className="space-y-2.5 pt-2 border-t border-ink-700">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
            Diagnostics
          </span>
          <div className="space-y-2">
            <Toggle
              label="KVM diagnostics log"
              checked={kvmSettings.debugLog ?? false}
              onChange={handleDebugLogToggle}
            />
            <p className="text-[11px] text-text-muted">
              Records keyboard hook decisions, engage/return/release events, slow callbacks (&gt;20ms), and foreground window details (rotates at 5 MB).
            </p>
            <div className="flex items-center gap-2">
              <div
                className="flex-1 truncate rounded-lg border border-ink-700 bg-ink-950 px-3 py-1.5 font-mono text-[11px] text-text"
                title={debugLogPath || "kvm-debug.log"}
              >
                {debugLogPath || "kvm-debug.log"}
              </div>
              <button
                type="button"
                onClick={() => void kvmOpenDebugLogFolder().catch(() => {})}
                className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong cursor-pointer shrink-0"
              >
                <FolderOpen className="h-3.5 w-3.5 text-text-muted" />
                <span>Open log folder</span>
              </button>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function ContinuitySettingsCard() {
  const kvmSettings = useKvmStore((s) => s.settings);
  const updateKvmSettings = useKvmStore((s) => s.updateSettings);
  const audioState = useKvmStore((s) => s.audioState);

  return (
    <Card className="rounded-xl border border-ink-700 bg-ink-900">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-sm font-semibold text-text-strong">
          <ArrowLeftRight className="h-4 w-4 text-gold-400" />
          Continuity
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-5">
        {/* Files & Pointer (SPEC-25a) */}
        <div className="space-y-2.5">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
            Files &amp; Pointer
          </span>
          <div className="space-y-2">
            <Toggle
              label="Drag & drop files across PCs"
              tag="Experimental"
              hint="Drag files across the seam. May not work with every app — copy and paste is more reliable."
              checked={kvmSettings.continuityDragDropExperimental}
              onChange={(checked) =>
                updateKvmSettings({ continuityDragDropExperimental: checked })
              }
            />
            <Toggle
              label="Copy-paste files across PCs"
              checked={kvmSettings.continuityFileClipboard}
              onChange={(checked) =>
                updateKvmSettings({ continuityFileClipboard: checked })
              }
            />
            <Toggle
              label="Sync pointer scheme to Linked PC"
              checked={kvmSettings.continuityPointerScheme}
              onChange={(checked) =>
                updateKvmSettings({ continuityPointerScheme: checked })
              }
            />
          </div>
          <div className="grid max-w-xl grid-cols-1 gap-4 pt-1 sm:grid-cols-2">
            <NumberField
              label="Staging size limit (MB)"
              hint="Files above this limit are saved to Downloads\AnyLeap_Transfers."
              value={kvmSettings.continuityStagingLimitMb}
              onChange={(v) =>
                updateKvmSettings({
                  continuityStagingLimitMb: Math.max(64, v),
                })
              }
            />
            <NumberField
              label="Clipboard prefetch limit (MB)"
              hint="Larger copied files wait until you click Fetch"
              value={kvmSettings.continuityClipPrefetchLimitMb}
              onChange={(v) =>
                updateKvmSettings({
                  continuityClipPrefetchLimitMb: Math.max(0, v),
                })
              }
            />
          </div>
        </div>

        {/* Audio & Media Keys (SPEC-25b) */}
        <div className="space-y-2.5 pt-2 border-t border-ink-700">
          <div className="flex items-center justify-between">
            <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
              Audio &amp; Media Keys
            </span>
            {audioState.streaming && (
              <span className="rounded border border-status-success/40 bg-status-success-bg px-1.5 py-0.5 font-mono text-[10px] text-status-success">
                Streaming
              </span>
            )}
          </div>
          <div className="space-y-2">
            <div className="flex flex-wrap items-start gap-x-6 gap-y-2">
              <Toggle
                label="Audio bridge (Linked PC → Main PC)"
                checked={kvmSettings.continuityAudioBridge}
                onChange={(checked) =>
                  updateKvmSettings({ continuityAudioBridge: checked })
                }
              />
              <Toggle
                label="Mute Linked PC while bridging"
                checked={kvmSettings.continuityMuteLinkedWhileBridging}
                disabled={audioState.linkedMute === "unsupported"}
                hint={
                  audioState.linkedMute === "unsupported"
                    ? "Not supported on the Linked PC"
                    : undefined
                }
                onChange={(checked) =>
                  updateKvmSettings({ continuityMuteLinkedWhileBridging: checked })
                }
              />
            </div>
            {audioState.streaming && (
              <div className="space-y-1 pl-6">
                <div className="h-1.5 w-full max-w-xs overflow-hidden rounded-full bg-ink-950 border border-ink-700">
                  <div
                    className="progress-bar-fill h-full w-full bg-gold-400"
                    style={{
                      transform: `scaleX(${Math.max(0, Math.min(1, audioState.level))})`,
                    }}
                  />
                </div>
              </div>
            )}
            <div className="space-y-1.5 pt-1">
              <div className="flex items-center justify-between text-xs">
                <label
                  htmlFor="continuity-audio-volume"
                  className="font-medium text-text"
                >
                  Bridged audio volume
                </label>
                <span className="font-mono text-gold-400 font-semibold">
                  {kvmSettings.continuityAudioVolume}%
                </span>
              </div>
              <input
                id="continuity-audio-volume"
                type="range"
                min={0}
                max={100}
                step={1}
                value={kvmSettings.continuityAudioVolume}
                onChange={(e) =>
                  updateKvmSettings({
                    continuityAudioVolume: Number(e.target.value),
                  })
                }
                className="w-full accent-gold-400 cursor-pointer"
              />
            </div>
            <Toggle
              label="Media keys follow active audio"
              checked={kvmSettings.continuityMediaKeysFollowAudio}
              onChange={(checked) =>
                updateKvmSettings({ continuityMediaKeysFollowAudio: checked })
              }
            />
          </div>
        </div>

        {/* Session & Open on Linked PC (SPEC-25c) */}
        <div className="space-y-2.5 pt-2 border-t border-ink-700">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
            Session &amp; Open on Linked PC
          </span>
          <div className="space-y-2">
            <Toggle
              label="Lock together"
              checked={kvmSettings.continuityLockTogether}
              onChange={(checked) =>
                updateKvmSettings({ continuityLockTogether: checked })
              }
            />
            <Toggle
              label="Sleep together"
              checked={kvmSettings.continuitySleepTogether}
              onChange={(checked) =>
                updateKvmSettings({ continuitySleepTogether: checked })
              }
            />
            <Toggle
              label='Explorer "Open on Linked PC" menu'
              checked={kvmSettings.continuityOpenOnLinkedMenu}
              onChange={(checked) =>
                updateKvmSettings({ continuityOpenOnLinkedMenu: checked })
              }
            />
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function NumberField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="space-y-1">
      <label className="text-xs font-medium text-text-muted">{label}</label>
      <input
        type="number"
        min={0}
        className={selectClass}
        value={value}
        onChange={(e) => onChange(Number(e.target.value) || 0)}
      />
      <p className="text-[11px] text-text-muted">{hint}</p>
    </div>
  );
}

function Toggle({
  label,
  tag,
  checked,
  disabled = false,
  hint,
  onChange,
}: {
  label: string;
  tag?: string;
  checked: boolean;
  disabled?: boolean;
  hint?: string;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="space-y-0.5">
      <label
        className={`flex items-center gap-2 text-sm ${
          disabled ? "cursor-not-allowed text-text-muted opacity-60" : "text-text cursor-pointer"
        }`}
      >
        <input
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={(e) => onChange(e.target.checked)}
          className="h-4 w-4 accent-gold-400"
        />
        <span>{label}</span>
        {tag && (
          <span className="rounded border border-gold-400/40 bg-ink-800 px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase tracking-wide text-gold-400">
            {tag}
          </span>
        )}
      </label>
      {hint && <p className="pl-6 text-[11px] text-text-muted">{hint}</p>}
    </div>
  );
}
