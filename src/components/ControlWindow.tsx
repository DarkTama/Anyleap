import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { GripHorizontal, X } from "lucide-react";
import { ControlBar } from "@/components/ControlBar";
import { mirrorRect } from "@/lib/tauri";
import {
  DEFAULT_CONTROL_CONFIG,
  loadControlConfig,
  type ControlConfig,
} from "@/lib/controlConfig";

const THICKNESS: Record<ControlConfig["size"], number> = { sm: 80, md: 92, lg: 112 };
const STEP: Record<ControlConfig["size"], number> = { sm: 56, md: 64, lg: 76 };

const countButtons = (c: ControlConfig) => Object.values(c.buttons).filter(Boolean).length;

/** Floating, always-on-top control window. Reads the user's control-bar config
 *  (dock side / size / buttons) and docks itself to the scrcpy mirror window. */
export function ControlWindow({ serial }: { serial: string }) {
  const [config, setConfig] = useState<ControlConfig>(DEFAULT_CONTROL_CONFIG);

  // Load config + live updates from the main window.
  useEffect(() => {
    loadControlConfig().then(setConfig).catch(() => {});
    const un = listen<ControlConfig>("control-config", (e) => setConfig(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const vertical =
    config.dock === "left" || config.dock === "right" || config.dock === "undocked";
  const orientation = vertical ? "vertical" : "horizontal";

  // Size the window from the config (logical px so it matches the CSS layout).
  useEffect(() => {
    const long = 48 + countButtons(config) * STEP[config.size];
    const thick = THICKNESS[config.size];
    const w = vertical ? thick : long;
    const h = vertical ? long : thick + 28;
    getCurrentWindow().setSize(new LogicalSize(w, h)).catch(() => {});
  }, [config, vertical]);

  // Dock to the mirror window and follow it (skip when undocked = free-floating).
  useEffect(() => {
    if (config.dock === "undocked") return;
    const dock = config.dock;
    const title = `AnyLeap — ${serial}`;
    const win = getCurrentWindow();
    let active = true;
    let visible = true;
    let lastX = Number.NaN;
    let lastY = Number.NaN;

    const tick = async () => {
      if (!active) return;
      try {
        const r = await mirrorRect(title);
        // Follow the mirror's focus: only show while an AnyLeap window (the
        // mirror or this strip) is foreground — hide when another app is active,
        // or when the mirror is minimized/gone.
        const active = !!r && !r.minimized && r.foreground.startsWith("AnyLeap");
        if (!r || !active) {
          if (visible) {
            visible = false;
            await win.hide();
          }
          return;
        }
        if (!visible) {
          visible = true;
          await win.show();
        }
        const sz = await win.outerSize(); // physical px
        const w = sz.width;
        const h = sz.height;
        let x: number;
        let y: number;
        if (dock === "right") {
          x = r.x + r.width;
          y = r.y;
        } else if (dock === "left") {
          x = r.x - w;
          y = r.y;
        } else if (dock === "top") {
          x = r.x;
          y = r.y - h;
        } else {
          x = r.x; // bottom
          y = r.y + r.height;
        }
        x = Math.max(r.workLeft, Math.min(x, r.workRight - w));
        y = Math.max(r.workTop, Math.min(y, r.workBottom - h));
        if (x !== lastX || y !== lastY) {
          lastX = x;
          lastY = y;
          await win.setPosition(new PhysicalPosition(x, y));
        }
      } catch {
        // mirror window not up yet / off a monitor — ignore
      }
    };

    const id = setInterval(tick, 50);
    void tick();
    return () => {
      active = false;
      clearInterval(id);
    };
  }, [serial, config.dock]);

  return (
    <div className="flex h-screen flex-col bg-zinc-900 text-zinc-50">
      <div
        data-tauri-drag-region
        className="flex cursor-move items-center justify-between border-b border-zinc-800 px-2 py-1"
      >
        <GripHorizontal className="pointer-events-none h-4 w-4 text-zinc-500" />
        <button
          onClick={() => getCurrentWindow().close()}
          className="rounded p-0.5 text-zinc-400 hover:bg-zinc-800"
          aria-label="Close"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
      <div className="flex-1 overflow-auto">
        <ControlBar serial={serial} config={config} orientation={orientation} />
      </div>
    </div>
  );
}
