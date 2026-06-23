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

const CODECS: Codec[] = ["h264", "h265", "av1"];
const PRESETS: QualityPreset[] = ["low", "medium", "high", "highest", "custom"];

export function SettingsPanel() {
  const settings = useAppStore((s) => s.settings);
  const preset = useAppStore((s) => s.preset);
  const setSettings = useAppStore((s) => s.setSettings);
  const setPreset = useAppStore((s) => s.setPreset);

  function applyPreset(p: QualityPreset) {
    setPreset(p);
    if (p !== "custom") setSettings({ ...settings, ...PRESET_BUNDLES[p] });
  }

  function update<K extends keyof CoreSettings>(key: K, value: CoreSettings[K]) {
    setSettings({ ...settings, [key]: value });
    setPreset("custom");
  }

  return (
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
