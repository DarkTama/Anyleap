import { useMemo } from "react";
import { Monitor } from "lucide-react";

interface DpiVisualizerProps {
  dpi: number;
  onChange: (dpi: number) => void;
}

const PRESETS = [
  { label: "120 - Micro", dpi: 120, desc: "Tiny icons, maximum multi-window space" },
  { label: "160 - Desktop", dpi: 160, desc: "Compact desktop apps, ideal for 1080p" },
  { label: "200 - Balanced", dpi: 200, desc: "Balanced scaling for 1080p & 1440p" },
  { label: "240 - Tablet", dpi: 240, desc: "Standard Android tablet default" },
  { label: "320 - Large", dpi: 320, desc: "Large touch-friendly phone sizing" },
];

export function DpiVisualizer({ dpi, onChange }: DpiVisualizerProps) {
  const preview = useMemo(() => {
    const scale = 160 / Math.max(100, Math.min(480, dpi));
    const iconSize = Math.max(16, Math.min(44, Math.round(30 * scale)));
    const fontSize = Math.max(8, Math.min(12, Math.round(10 * scale)));
    const taskbarHeight = Math.max(18, Math.min(34, Math.round(22 * scale)));

    return {
      iconSize,
      fontSize,
      taskbarHeight,
      summary:
        dpi <= 140
          ? "Micro Workspace: Maximum multi-window area"
          : dpi <= 180
          ? "Desktop Clean: Comfortable mouse navigation"
          : dpi <= 220
          ? "Balanced HD: Clear icons with ample space"
          : dpi <= 260
          ? "Standard Tablet: Android tablet scaling"
          : "Touch Large: Big icons, phone-like targets",
    };
  }, [dpi]);

  return (
    <div className="space-y-3 rounded-xl border border-[#222226] bg-[#141417] p-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Monitor className="h-4 w-4 text-zinc-300" />
          <span className="text-xs font-semibold text-zinc-200">Flex Display Density (DPI)</span>
        </div>
        <div className="flex items-baseline gap-1 font-mono">
          <span className="text-sm font-bold text-zinc-100">{dpi}</span>
          <span className="text-[10px] text-zinc-500">DPI</span>
        </div>
      </div>

      {/* Interactive Range Slider */}
      <div className="space-y-1.5">
        <input
          type="range"
          min={120}
          max={360}
          step={10}
          value={dpi}
          onChange={(e) => onChange(Number(e.target.value))}
          className="h-1.5 w-full cursor-pointer appearance-none rounded-lg bg-[#222226] accent-white"
        />
        <div className="flex justify-between text-[9px] font-mono text-zinc-500">
          <span>120 (Smallest)</span>
          <span>160 (Desktop)</span>
          <span>240 (Tablet)</span>
          <span>360 (Largest)</span>
        </div>
      </div>

      {/* Preset Chips */}
      <div className="flex flex-wrap gap-1.5 pt-0.5">
        {PRESETS.map((p) => {
          const active = Math.abs(p.dpi - dpi) < 10;
          return (
            <button
              key={p.dpi}
              type="button"
              onClick={() => onChange(p.dpi)}
              className={`btn-tactile rounded-md px-2 py-1 text-[10px] font-mono cursor-pointer ${
                active
                  ? "bg-[#222228] text-zinc-100 border border-[#52525b] font-semibold"
                  : "bg-[#18181c] text-zinc-400 hover:text-zinc-200 border border-[#27272a]"
              }`}
            >
              {p.label}
            </button>
          );
        })}
      </div>

      {/* Live Mock Screen Preview */}
      <div className="space-y-1.5 pt-1">
        <div className="flex items-center justify-between text-[11px]">
          <span className="text-zinc-400 font-medium">Virtual Desktop Canvas Scale</span>
          <span className="text-[10px] font-mono text-zinc-500">{preview.summary}</span>
        </div>

        <div className="relative flex h-24 w-full flex-col justify-between overflow-hidden rounded-lg border border-[#222226] bg-[#0c0c0e] p-2.5">
          {/* App Icons Row */}
          <div className="flex items-start gap-3">
            {[
              { label: "Browser", color: "bg-blue-500/20 text-blue-400 border-blue-500/30" },
              { label: "Files", color: "bg-amber-500/20 text-amber-400 border-amber-500/30" },
              { label: "Notes", color: "bg-emerald-500/20 text-emerald-400 border-emerald-500/30" },
              { label: "Terminal", color: "bg-purple-500/20 text-purple-400 border-purple-500/30" },
            ].map((app) => (
              <div key={app.label} className="flex flex-col items-center gap-1">
                <div
                  className={`grid place-items-center rounded-lg border ${app.color} transition-all duration-150`}
                  style={{
                    width: `${preview.iconSize}px`,
                    height: `${preview.iconSize}px`,
                  }}
                >
                  <span style={{ fontSize: `${Math.max(7, preview.fontSize - 1)}px` }}>●</span>
                </div>
                <span
                  className="font-medium text-zinc-400 truncate max-w-[48px]"
                  style={{ fontSize: `${preview.fontSize}px` }}
                >
                  {app.label}
                </span>
              </div>
            ))}
          </div>

          {/* Virtual Bottom Taskbar */}
          <div
            className="flex items-center justify-between border-t border-[#1f1f23] bg-[#121215] px-2 text-zinc-400"
            style={{ height: `${preview.taskbarHeight}px` }}
          >
            <span style={{ fontSize: `${preview.fontSize}px` }}>Apps</span>
            <span className="font-mono" style={{ fontSize: `${preview.fontSize}px` }}>
              100% · 12:00
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
