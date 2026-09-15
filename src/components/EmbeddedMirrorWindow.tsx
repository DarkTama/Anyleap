import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X, GripHorizontal } from "lucide-react";
import { embedMirror, resizeEmbeddedMirror } from "@/lib/tauri";
import { ControlBar } from "./ControlBar";
import { DEFAULT_CONTROL_CONFIG, loadControlConfig, type ControlConfig } from "@/lib/controlConfig";

export function EmbeddedMirrorWindow({ serial }: { serial: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [config, setConfig] = useState<ControlConfig>(DEFAULT_CONTROL_CONFIG);
  const win = getCurrentWindow();

  useEffect(() => {
    loadControlConfig().then(setConfig).catch(() => {});
  }, []);

  useEffect(() => {
    const label = win.label;
    let active = true;

    // Trigger embed reparenting
    embedMirror(label, serial).catch((err) => {
      console.warn("Failed to embed scrcpy mirror:", err);
    });

    const updateSize = () => {
      if (!active || !containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const scale = window.devicePixelRatio || 1;
      const w = Math.round(rect.width * scale);
      const h = Math.round(rect.height * scale);
      const x = Math.round(rect.left * scale);
      const y = Math.round(rect.top * scale);
      resizeEmbeddedMirror(label, serial, w, h, x, y).catch(() => {});
    };

    const ro = new ResizeObserver(() => {
      updateSize();
    });

    if (containerRef.current) {
      ro.observe(containerRef.current);
    }

    const interval = setInterval(updateSize, 300);

    return () => {
      active = false;
      ro.disconnect();
      clearInterval(interval);
    };
  }, [serial, win.label]);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-zinc-950 text-zinc-100 select-none">
      {/* Custom Title Bar */}
      <div
        data-tauri-drag-region
        className="flex h-9 items-center justify-between border-b border-zinc-800/80 bg-zinc-900 px-3 shrink-0"
      >
        <div className="flex items-center gap-2 pointer-events-none">
          <GripHorizontal className="h-3.5 w-3.5 text-zinc-500" />
          <span className="text-xs font-medium text-zinc-200">AnyLeap — {serial}</span>
        </div>
        <div className="flex items-center gap-1">
          <button
            onClick={() => win.minimize()}
            className="rounded p-1 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200"
            aria-label="Minimize"
          >
            <Minus className="h-3 w-3" />
          </button>
          <button
            onClick={() => win.toggleMaximize()}
            className="rounded p-1 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200"
            aria-label="Maximize"
          >
            <Square className="h-3 w-3" />
          </button>
          <button
            onClick={() => win.close()}
            className="rounded p-1 text-zinc-400 hover:bg-red-900/50 hover:text-red-300"
            aria-label="Close"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      {/* Main Viewport & Integrated Sidebar */}
      <div className="flex flex-1 overflow-hidden">
        {/* Scrcpy Viewport Container */}
        <div
          ref={containerRef}
          id="mirror-viewport"
          className="flex-1 h-full bg-black relative"
        />

        {/* Flush Right Toolbar Sidebar */}
        <div className="w-12 shrink-0 border-l border-zinc-800/80 bg-zinc-900/95 flex flex-col items-center overflow-y-auto py-1.5">
          <ControlBar
            serial={serial}
            config={config}
            orientation="vertical"
            showOrientToggle={true}
            showSwipeScroll={true}
          />
        </div>
      </div>
    </div>
  );
}
