import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useAppStore } from "@/store/useAppStore";
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

  return (
    <div className="space-y-4">
    <Card>
      <CardHeader>
        <CardTitle>Settings</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="space-y-1">
          <label className="text-xs font-medium text-zinc-500">Quality preset</label>
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
          <p className="text-xs text-zinc-500">{PRESET_HINTS[preset]}</p>
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
            <label className="text-xs font-medium text-zinc-500">Codec</label>
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

        <div className="space-y-2 border-t border-zinc-100 pt-3 dark:border-zinc-800/60">
          <label className="text-xs font-medium text-zinc-500">App</label>
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
            <label className="text-xs font-medium text-zinc-500">Dock position</label>
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
            <label className="text-xs font-medium text-zinc-500">Size</label>
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
          <label className="text-xs font-medium text-zinc-500">Buttons</label>
          <div className="grid grid-cols-2 gap-2">
            {BUTTON_IDS.map((id) => (
              <label key={id} className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  className="h-4 w-4"
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

        <p className="text-[11px] text-zinc-400">
          Applies to the floating control strip and the in-app controls; changes take effect
          immediately.
        </p>
      </CardContent>
    </Card>
    </div>
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
      <label className="text-xs font-medium text-zinc-500">{label}</label>
      <input
        type="number"
        min={0}
        className={selectClass}
        value={value}
        onChange={(e) => onChange(Number(e.target.value) || 0)}
      />
      <p className="text-[11px] text-zinc-400">{hint}</p>
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
    <label className="flex items-center gap-2 text-sm">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4"
      />
      {label}
    </label>
  );
}
