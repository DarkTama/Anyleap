import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { FolderOpen, LayoutGrid, Monitor, RefreshCw, Sparkles, Laptop } from "lucide-react";
import { getSystemResolution } from "@/lib/tauri";
import { useAppStore } from "@/store/useAppStore";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import {
  PRESET_BUNDLES,
  PRESET_HINTS,
  PRESET_LABELS,
  type Codec,
  type CoreSettings,
  type QualityPreset,
} from "@/lib/types";
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
import { saveAppPrefs, saveQuality, type AppPrefs } from "@/lib/persist";
import { useKvmStore } from "@/stores/kvmStore";
import {
  kvmGetDebugLogPath,
  kvmOpenDebugLogFolder,
  kvmSetSyncSettings,
} from "@/services/kvmService";

const CODECS: Codec[] = ["h264", "h265", "av1"];
const SIZE_LABELS: Record<ControlConfig["size"], string> = {
  sm: "Small",
  md: "Medium",
  lg: "Large",
};
const PRESETS: QualityPreset[] = ["low", "medium", "high", "highest", "custom"];

export function SettingsPanel() {
  const settings = useAppStore((s) => s.settings);
  const preset = useAppStore((s) => s.preset);
  const setSettings = useAppStore((s) => s.setSettings);
  const setPreset = useAppStore((s) => s.setPreset);

  function applyPreset(p: QualityPreset) {
    const next = p !== "custom" ? { ...settings, ...PRESET_BUNDLES[p] } : settings;
    setPreset(p);
    setSettings(next);
    void saveQuality(next, p);
  }

  function update<K extends keyof CoreSettings>(key: K, value: CoreSettings[K]) {
    const next = { ...settings, [key]: value };
    setSettings(next);
    setPreset("custom");
    void saveQuality(next, "custom");
  }

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
    <Card>
      <CardHeader>
        <CardTitle>Settings</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="space-y-1">
          <label className="text-xs font-medium text-text-muted">Quality preset</label>
          <select
            className={selectClass}
            value={preset}
            onChange={(e) => applyPreset(e.target.value as QualityPreset)}
          >
            {PRESETS.map((p) => (
              <option key={p} value={p} className={optionClass}>
                {PRESET_LABELS[p]}
              </option>
            ))}
          </select>
          <p className="text-xs text-text-muted">{PRESET_HINTS[preset]}</p>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <NumberField
            label="Max resolution"
            hint="Long side in px; 0 = native"
            value={settings.maxSize}
            onChange={(v) => update("maxSize", v)}
          />
          <NumberField
            label="Bitrate (Mbps)"
            hint="Higher = sharper, needs better Wi-Fi"
            value={Math.round(settings.videoBitRate / 1_000_000)}
            onChange={(v) => update("videoBitRate", v * 1_000_000)}
          />
          <NumberField
            label="Max FPS"
            hint="60 smooth; 30 saves bandwidth"
            value={settings.maxFps}
            onChange={(v) => update("maxFps", v)}
          />
          <div className="space-y-1">
            <label className="text-xs font-medium text-text-muted">Codec</label>
            <select
              className={selectClass}
              value={settings.videoCodec}
              onChange={(e) => update("videoCodec", e.target.value as Codec)}
            >
              {CODECS.map((c) => (
                <option key={c} value={c} className={optionClass}>
                  {c}
                </option>
              ))}
            </select>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-2">
          <Toggle
            label="Keep awake"
            checked={settings.stayAwake}
            onChange={(v) => update("stayAwake", v)}
          />
          <Toggle
            label="Turn screen off"
            checked={settings.turnScreenOff}
            onChange={(v) => update("turnScreenOff", v)}
          />
          <Toggle
            label="Fullscreen"
            checked={settings.fullscreen}
            onChange={(v) => update("fullscreen", v)}
          />
          <Toggle
            label="Show touches"
            checked={settings.showTouches}
            onChange={(v) => update("showTouches", v)}
          />
          <Toggle
            label="No audio"
            checked={settings.noAudio}
            onChange={(v) => update("noAudio", v)}
          />
          <Toggle
            label="View only"
            checked={settings.noControl}
            onChange={(v) => update("noControl", v)}
          />
          <Toggle
            label="Disable virtual keyboard"
            checked={settings.noKeyboardIme}
            onChange={(v) => update("noKeyboardIme", v)}
          />
        </div>

        <div className="space-y-2 border-t border-ink-700 pt-3">
          <label className="text-xs font-medium text-text-muted">Display</label>
          <div className="space-y-1">
            <Toggle
              label="Flex display (desktop mode)"
              checked={settings.flexDisplay}
              onChange={(v) => update("flexDisplay", v)}
            />
            <p className="text-[11px] text-text-muted">
              Virtual display that continuously resizes with the window (Android 10+)
            </p>
          </div>
          <div className="space-y-1">
            <Toggle
              label="Embedded mirror (integrated sidebar)"
              checked={settings.embedded ?? false}
              onChange={(v) => update("embedded", v)}
            />
            <p className="text-[11px] text-text-muted">
              Embeds mirror viewport and toolbar into a single native window (Escrcpy style)
            </p>
          </div>
          {settings.flexDisplay && (() => {
            const rawSize = settings.flexDisplaySize || "";
            const dpiMatch = rawSize.match(/\/(\d+)/);
            const parsedDpi = dpiMatch ? Number.parseInt(dpiMatch[1], 10) : 240;
            const iconSize = Math.max(20, Math.min(48, Math.round(28 * (parsedDpi / 200))));
            const autoDetectResolution = async () => {
              try {
                const [w, h] = await getSystemResolution();
                update("flexDisplaySize", `${w}x${h}/200`);
              } catch (e) {
                console.error(e);
              }
            };
            return (
              <div className="space-y-2">
                <div className="flex gap-2">
                  <input
                    type="text"
                    className={selectClass}
                    value={settings.flexDisplaySize}
                    placeholder="Auto — or e.g. 1920x1080/200"
                    onChange={(e) => update("flexDisplaySize", e.target.value)}
                  />
                  <button
                    type="button"
                    onClick={autoDetectResolution}
                    className="btn-tactile flex items-center gap-1 rounded border border-ink-700 bg-ink-800 px-2 text-[11px] text-text hover:border-text-muted hover:text-text-strong shrink-0"
                    title="Detect PC native monitor resolution"
                  >
                    <Monitor className="h-3 w-3" />
                    Auto PC Res
                  </button>
                </div>

                {/* DPI Scale explanation & Live Icon Preview */}
                <div className="flex items-center gap-3 rounded border border-ink-700 bg-ink-950 p-2.5">
                  <div className="flex flex-col items-center gap-1 shrink-0">
                    <div
                      className="flex items-center justify-center rounded-xl border border-gold-400/50 bg-ink-800 text-gold-400"
                      style={{ width: `${iconSize}px`, height: `${iconSize}px` }}
                    >
                      <LayoutGrid className="w-1/2 h-1/2" />
                    </div>
                    <span className="text-[10px] text-text-muted">App</span>
                  </div>
                  <div className="text-xs text-text-muted">
                    <span className="font-semibold text-text-strong">{parsedDpi} DPI</span> icon size preview
                    <p className="text-[10px] text-text-muted mt-0.5">
                      {parsedDpi <= 160
                        ? "Compact: small icons, maximum desktop workspace"
                        : parsedDpi <= 200
                        ? "Medium: balanced app and desktop scaling"
                        : parsedDpi <= 240
                        ? "Standard: tablet default sizing"
                        : "Large: touch-friendly oversized icons"}
                    </p>
                  </div>
                </div>

                {/* Preset chips */}
                <div className="flex flex-wrap gap-1.5 pt-0.5">
                  {[
                    { label: "1080p Compact (160 DPI)", val: "1920x1080/160" },
                    { label: "1080p Balanced (200 DPI)", val: "1920x1080/200" },
                    { label: "1080p Standard (240 DPI)", val: "1920x1080/240" },
                    { label: "1440p HiDPI (200 DPI)", val: "2560x1440/200" },
                  ].map((p) => (
                    <button
                      key={p.val}
                      type="button"
                      onClick={() => update("flexDisplaySize", p.val)}
                      className={`btn-tactile rounded px-1.5 py-0.5 text-[10px] ${
                        settings.flexDisplaySize === p.val
                          ? "border border-gold-400 bg-ink-800 text-gold-400 font-medium"
                          : "border border-ink-700 bg-ink-950 text-text-muted hover:bg-ink-800 hover:text-text-strong"
                      }`}
                    >
                      {p.label}
                    </button>
                  ))}
                </div>
                <p className="text-[11px] text-text-muted">
                  Resolution as WxH, and /dpi. Lower DPI gives smaller icons and more screen space.
                </p>

                <Toggle
                  label="Hide virtual taskbar"
                  checked={settings.hideVirtualTaskbar ?? false}
                  onChange={(v) => update("hideVirtualTaskbar", v)}
                />
                <p className="text-[11px] text-text-muted">
                  Hides Android navigation bar / taskbar on the virtual desktop display
                </p>
              </div>
            );
          })()}
          <Toggle
            label="Unlock aspect ratio"
            checked={settings.noWindowAspectRatioLock}
            onChange={(v) => update("noWindowAspectRatioLock", v)}
          />
          <div className="space-y-1">
            <label className="text-xs font-medium text-text-muted">Render fit</label>
            <select
              className={selectClass}
              value={settings.renderFit}
              onChange={(e) => update("renderFit", e.target.value as CoreSettings["renderFit"])}
            >
              <option value="letterbox" className={optionClass}>Letterbox</option>
              <option value="unscaled" className={optionClass}>Unscaled</option>
              <option value="stretched" className={optionClass}>Stretched</option>
            </select>
          </div>
        </div>

        <div className="space-y-2 border-t border-ink-700 pt-3">
          <label className="text-xs font-medium text-text-muted">App</label>
          <Toggle
            label="Minimize to tray on close"
            checked={appPrefs.minimizeToTrayOnClose}
            onChange={(v) => updateAppPrefs({ ...appPrefs, minimizeToTrayOnClose: v })}
          />
          <Toggle
            label="Check for updates on launch"
            checked={appPrefs.checkUpdates}
            onChange={(v) => updateAppPrefs({ ...appPrefs, checkUpdates: v })}
          />
          <Toggle
            label="Desktop pop-ups for phone notifications"
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
      kvmSettings.syncFileTransfer
    ).catch(() => {});
  }, [
    kvmSettings.syncClipboardText,
    kvmSettings.syncClipboardImage,
    kvmSettings.syncFileTransfer,
  ]);

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
            <Toggle
              label="File Transfer Drop Sync"
              checked={kvmSettings.syncFileTransfer}
              onChange={(checked) =>
                updateKvmSettings({ syncFileTransfer: checked })
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
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-sm text-text">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4 accent-gold-400"
      />
      {label}
    </label>
  );
}
