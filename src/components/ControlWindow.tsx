import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { Gamepad2, GripHorizontal, Minimize2, X } from "lucide-react";
import { ControlBar } from "@/components/ControlBar";
import { mirrorRect } from "@/lib/tauri";
import { loadQuality } from "@/lib/persist";
import {
  DEFAULT_CONTROL_CONFIG,
  loadControlConfig,
  saveControlConfig,
  type ControlConfig,
} from "@/lib/controlConfig";

const THICKNESS: Record<ControlConfig["size"], number> = { sm: 80, md: 92, lg: 112 };
const STEP: Record<ControlConfig["size"], number> = { sm: 56, md: 64, lg: 76 };
/** Logical size of the collapsed round button window. */
const COLLAPSED = 44;

const countButtons = (c: ControlConfig) => Object.values(c.buttons).filter(Boolean).length;

/** Floating, always-on-top control window. Reads the user's control-bar config
 *  (dock side / size / buttons) and docks itself to the scrcpy mirror window. */
export function ControlWindow({ serial }: { serial: string }) {
  const [config, setConfig] = useState<ControlConfig>(DEFAULT_CONTROL_CONFIG);
  // Rotate only affects the physical display, so hide it while mirroring a
  // flex (virtual) display.
  const [flexDisplay, setFlexDisplay] = useState(false);

  // Load config + live updates from the main window.
  useEffect(() => {
    loadControlConfig().then(setConfig).catch(() => {});
    loadQuality()
      .then((q) => setFlexDisplay(q?.settings.flexDisplay ?? false))
      .catch(() => {});
    const un = listen<ControlConfig>("control-config", (e) => setConfig(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const vertical =
    config.dock === "left" || config.dock === "right" || config.dock === "undocked";
  const orientation = vertical ? "vertical" : "horizontal";
  const collapsed = config.collapsed;

  // Ref so the 50ms docking tick sees the live value without re-arming.
  const collapsedRef = useRef(collapsed);
  collapsedRef.current = collapsed;

  const setCollapsed = (v: boolean) => {
    setConfig((c) => {
      const next = { ...c, collapsed: v };
      saveControlConfig(next).catch(() => {});
      return next;
    });
  };

  // Size the window from the config (logical px so it matches the CSS layout).
  useEffect(() => {
    const long = 48 + countButtons(config) * STEP[config.size];
    const thick = THICKNESS[config.size];
    const w = collapsed ? COLLAPSED : vertical ? thick : long;
    const h = collapsed ? COLLAPSED : vertical ? long : thick + 28;
    getCurrentWindow().setSize(new LogicalSize(w, h)).catch(() => {});
  }, [config, vertical, collapsed]);

  // Dock to the mirror window and follow it (skip when undocked = free-floating).
  useEffect(() => {
    if (config.dock === "undocked") return;
    const dock = config.dock;
    const title = `AnyLeap — ${serial}`;
    const win = getCurrentWindow();
    let active = true;
    let visible = true;
    let inFlight = false;
    let lastX = Number.NaN;
    let lastY = Number.NaN;

    const tick = async () => {
      if (!active || inFlight) return;
      inFlight = true;
      try {
        const r = await mirrorRect(title);
        const shouldShow = !!r && !r.minimized && r.foreground.startsWith("AnyLeap");
        if (!r || !shouldShow) {
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
        const sz = await win.outerSize();
        const w = sz.width;
        const h = sz.height;

        // Latch BESIDE the mirror's window border.
        // If hitting screen edge, gracefully flip inside.
        const cRight = r.clientX + r.clientWidth;
        const cBottom = r.clientY + r.clientHeight;
        const centerX = r.clientX + Math.round((r.clientWidth - w) / 2);
        const centerY = r.clientY + Math.round((r.clientHeight - h) / 2);
        let x: number;
        let y: number;
        if (dock === "right") {
          x = r.x + r.width;
          if (x + w > r.workRight) {
            x = cRight - w;
          }
          y = collapsedRef.current ? centerY : r.clientY;
        } else if (dock === "left") {
          x = r.x - w;
          if (x < r.workLeft) {
            x = r.clientX;
          }
          y = collapsedRef.current ? centerY : r.clientY;
        } else if (dock === "top") {
          x = collapsedRef.current ? centerX : r.clientX;
          y = r.y - h;
          if (y < r.workTop) {
            y = r.clientY;
          }
        } else {
          x = collapsedRef.current ? centerX : r.clientX;
          y = r.y + r.height;
          if (y + h > r.workBottom) {
            y = cBottom - h;
          }
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
      } finally {
        inFlight = false;
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
    <div
      className={
        collapsed
          ? "flex h-screen w-screen items-center justify-center bg-transparent"
          : "flex h-screen flex-col rounded-xl border border-zinc-700/60 bg-zinc-900/90 shadow-2xl backdrop-blur-md text-zinc-50"
      }
    >
      {collapsed ? (
        <button
          onClick={() => setCollapsed(false)}
          className="flex h-10 w-10 items-center justify-center rounded-full border border-zinc-700 bg-zinc-900/90 text-zinc-200 shadow-lg transition-transform hover:scale-105 hover:bg-zinc-800"
          aria-label="Expand controls"
          title="Expand controls"
        >
          <Gamepad2 className="h-5 w-5" />
        </button>
      ) : (
        <div
          data-tauri-drag-region
          className="flex cursor-move items-center justify-between border-b border-zinc-800/80 px-2.5 py-1.5"
        >
          <GripHorizontal className="pointer-events-none h-3.5 w-3.5 text-zinc-500" />
          <div className="flex items-center gap-1">
            <button
              onClick={() => setCollapsed(true)}
              className="rounded p-1 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200"
              aria-label="Collapse controls"
              title="Collapse to button"
            >
              <Minimize2 className="h-3.5 w-3.5" />
            </button>
            <button
              onClick={() => getCurrentWindow().close()}
              className="rounded p-1 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200"
              aria-label="Close"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      )}
      <div className={collapsed ? "hidden" : "flex-1 overflow-auto p-1"}>
        <ControlBar
          serial={serial}
          config={config}
          orientation={orientation}
          showOrientToggle={!flexDisplay}
          showSwipeScroll
        />
      </div>
    </div>
  );
}
