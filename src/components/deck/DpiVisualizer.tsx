import { useMemo } from "react";
import { Monitor, LayoutGrid, Clock, Wifi, Battery } from "lucide-react";

interface DpiVisualizerProps {
  dpi: number;
  onChange: (dpi: number) => void;
}

const PRESETS = [
  { label: "120 - Micro", dpi: 120, desc: "Ultra small icons, maximum screen real estate" },
  { label: "160 - Desktop", dpi: 160, desc: "Compact desktop apps, ideal for 1080p" },
  { label: "200 - Balanced", dpi: 200, desc: "Balanced scaling for 1080p & 1440p" },
  { label: "240 - Tablet", dpi: 240, desc: "Standard Android tablet default" },
  { label: "320 - Touch", dpi: 320, desc: "Large touch-friendly phone sizing" },
];

export function DpiVisualizer({ dpi, onChange }: DpiVisualizerProps) {
  // Compute mock app icon & taskbar sizing in inverse relation to DPI (Android density logic)
  const preview = useMemo(() => {
    // 160 dpi is baseline (1.0x). 240 is 1.5x. Lower dpi = more elements fit = smaller icon px
    const scale = 160 / Math.max(100, Math.min(480, dpi));
    const iconSize = Math.max(16, Math.min(48, Math.round(32 * scale)));
    const fontSize = Math.max(8, Math.min(13, Math.round(10 * scale)));
    const taskbarHeight = Math.max(18, Math.min(36, Math.round(24 * scale)));

    return {
      iconSize,
      fontSize,
      taskbarHeight,
      summary:
        dpi <= 140
          ? "Micro Workspace: Tiny icons, maximum multi-window area"
          : dpi <= 180
          ? "Desktop Clean: Small icons, comfortable mouse navigation"
          : dpi <= 220
          ? "Balanced HD: Clear icons with ample app layout space"
          : dpi <= 260
          ? "Standard Tablet: Android default tablet scaling"
          : "Touch Large: Big icons, phone-like touch targets",
    };
  }, [dpi]);

  return (
    <div className="space-y-3 rounded-xl border border-zinc-800/80 bg-zinc-900/60 p-3.5 backdrop-blur-sm">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Monitor className="h-4 w-4 text-cyan-400" />
          <span className="text-xs font-semibold text-zinc-200">Flex Display Density (DPI)</span>
        </div>
        <div className="flex items-baseline gap-1">
          <span className="font-mono text-sm font-bold text-cyan-300">{dpi}</span>
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
          className="h-1.5 w-full cursor-pointer appearance-none rounded-lg bg-zinc-800 accent-cyan-400"
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
              className={`rounded-md px-2 py-1 text-[10px] font-medium transition-all ${
                active
                  ? "bg-cyan-500/20 text-cyan-300 border border-cyan-500/40 shadow-sm"
                  : "bg-zinc-800/80 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200 border border-transparent"
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
          <span className="text-zinc-400 font-medium">Live Icon & Taskbar Preview</span>
          <span className="text-[10px] text-zinc-500">{preview.summary}</span>
        </div>

        <div className="relative flex h-32 w-full flex-col justify-between overflow-hidden rounded-lg border border-zinc-800 bg-gradient-to-br from-zinc-950 via-zinc-900 to-zinc-950 p-2.5 shadow-inner">
          {/* Mock Desktop Icons Grid */}
          <div className="flex flex-wrap items-start gap-4">
            {[
              { name: "Browser", gradient: "from-sky-500 to-blue-600" },
              { name: "Terminal", gradient: "from-emerald-500 to-teal-700" },
              { name: "Gallery", gradient: "from-violet-500 to-purple-700" },
              { name: "Files", gradient: "from-amber-500 to-orange-600" },
            ].map((app) => (
              <div key={app.name} className="flex flex-col items-center gap-1">
                <div
                  className={`flex items-center justify-center rounded-lg bg-gradient-to-tr ${app.gradient} text-white shadow-md transition-all duration-200`}
                  style={{
                    width: `${preview.iconSize}px`,
                    height: `${preview.iconSize}px`,
                  }}
                >
                  <LayoutGrid className="w-1/2 h-1/2 opacity-90" />
                </div>
                <span
                  className="font-medium text-zinc-300 transition-all duration-200 text-center select-none"
                  style={{ fontSize: `${preview.fontSize}px` }}
                >
                  {app.name}
                </span>
              </div>
            ))}
          </div>

          {/* Mock Taskbar Dock */}
          <div
            className="flex items-center justify-between rounded border border-zinc-800/80 bg-zinc-900/90 px-2 transition-all duration-200"
            style={{ height: `${preview.taskbarHeight}px` }}
          >
            <div className="flex items-center gap-1.5">
              <div className="h-2 w-2 rounded-full bg-cyan-400 animate-pulse" />
              <span className="text-[9px] font-semibold text-zinc-400">Desktop</span>
            </div>
            <div className="flex items-center gap-2 text-[9px] text-zinc-500">
              <Wifi className="h-2.5 w-2.5" />
              <Battery className="h-2.5 w-2.5" />
              <span className="flex items-center gap-0.5">
                <Clock className="h-2.5 w-2.5" />
                12:00
              </span>
            </div>
          </div>
        </div>
      </div>

      <p className="text-[11px] leading-relaxed text-zinc-400">
        <strong className="text-zinc-300">Tip:</strong> Android density logic is inverted from monitor DPI. Lower numbers give smaller icons and more workspace; higher numbers give larger touch targets.
      </p>
    </div>
  );
}
