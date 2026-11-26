import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize, PhysicalPosition } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { Gamepad2, GripHorizontal, Minimize2, X } from "lucide-react";
import { ControlBar } from "@/components/ControlBar";
import { controlBarGroups, DEFAULT_CONTROL_BAR_STATE } from "@/lib/controlBarModel";
import { listSessions, mirrorRect, onSessionExited, onSessionStarted } from "@/lib/tauri";
import type { DisplayMode, ModeDefaultsV2 } from "@/lib/modeSettings";
import { loadModeDefaults } from "@/lib/persist";
import { useAppStore } from "@/store/useAppStore";
import {
  DEFAULT_CONTROL_CONFIG,
  loadControlConfig,
  saveControlConfig,
  type ControlConfig,
} from "@/lib/controlConfig";

/** Fixed 40 px icon buttons (SPEC-28 phase 3): the strip is 40 + paddings thick. */
const THICKNESS = 76;
const BTN_STEP = 46;
const DIVIDER_STEP = 8;
/** Logical size of the collapsed round button window. */
const COLLAPSED = 44;

/** Length of the strip along its dock axis, from the grouped model (worst case: in a call). */
function stripLength(c: ControlConfig, flexDisplay: boolean): number {
  const model = controlBarGroups({ ...c, collapsed: false }, "display", {
    ...DEFAULT_CONTROL_BAR_STATE,
    inCall: true,
    showOrient: !flexDisplay,
  });
  const items = model.groups.reduce((n, g) => n + g.items.length, 0) + (model.more.length > 0 ? 1 : 0);
  const dividers = Math.max(0, model.groups.length - 1) + (model.more.length > 0 ? 1 : 0);
  return 48 + items * BTN_STEP + dividers * DIVIDER_STEP;
}

/** Floating, always-on-top control window. Reads the user's control-bar config
 *  (dock side / size / buttons) and docks itself to the scrcpy mirror window. */
export function ControlWindow({
  serial,
  flexDisplay = false,
  displayMode,
}: {
  serial: string;
  /** True when the running session was launched from the Flex mode (SPEC-28). */
  flexDisplay?: boolean;
  displayMode?: DisplayMode;
}) {
  const [config, setConfig] = useState<ControlConfig>(DEFAULT_CONTROL_CONFIG);
  const activeDisplayMode: DisplayMode = displayMode ?? (flexDisplay ? "flex" : "mirror");
  // Rotate only affects the physical display, so it is hidden while mirroring a
  // flex (virtual) display.

  // Load config, modeDefaults, and session state + live updates from the main window.
  useEffect(() => {
    loadControlConfig().then(setConfig).catch(() => {});
    loadModeDefaults()
      .then((m) => useAppStore.getState().setModeDefaults(m.modeDefaults, m.notice))
      .catch(() => {});
    listSessions()
      .then((sessions) => {
        const st = useAppStore.getState();
        st.setSessions(sessions);
        st.setCurrentMode(activeDisplayMode);
        const found = sessions.find((s) => s.serial === serial && s.mode !== "camera");
        if (found) st.upsertSession(found, activeDisplayMode);
      })
      .catch(() => {});
    const un = listen<ControlConfig>("control-config", (e) => setConfig(e.payload));
    const unModes = listen<ModeDefaultsV2>("mode-defaults", (e) => {
      if (e.payload) useAppStore.getState().setModeDefaults(e.payload);
    });
    const unStarted = onSessionStarted((e) => {
      if (e.payload.serial === serial) {
        useAppStore
          .getState()
          .upsertSession(e.payload, e.payload.mode === "camera" ? "camera" : activeDisplayMode);
      }
    });
    const unExited = onSessionExited((e) => {
      useAppStore.getState().removeSession(e.payload.id);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
      unModes.then((f) => f()).catch(() => {});
      unStarted.then((f) => f()).catch(() => {});
      unExited.then((f) => f()).catch(() => {});
    };
  }, [serial, activeDisplayMode]);

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
    const long = stripLength(config, flexDisplay);
    const thick = THICKNESS;
    const w = collapsed ? COLLAPSED : vertical ? thick : long;
    const h = collapsed ? COLLAPSED : vertical ? long : thick + 28;
    getCurrentWindow().setSize(new LogicalSize(w, h)).catch(() => {});
  }, [config, vertical, collapsed, flexDisplay]);

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
          : "flex h-screen flex-col rounded-xl border border-ink-700 bg-ink-900 shadow-elevated text-text-strong"
      }
    >
      {collapsed ? (
        <button
          onClick={() => setCollapsed(false)}
          className="btn-tactile flex h-10 w-10 items-center justify-center rounded-full border border-gold-400/60 bg-ink-900 text-gold-400 shadow-elevated hover:bg-ink-800"
          aria-label="Expand controls"
          title="Expand controls"
        >
          <Gamepad2 className="h-5 w-5" />
        </button>
      ) : (
        <div
          data-tauri-drag-region
          className="flex cursor-move items-center justify-between border-b border-ink-700 px-2.5 py-1.5"
        >
          <GripHorizontal className="pointer-events-none h-3.5 w-3.5 text-text-muted" />
          <div className="flex items-center gap-1">
            <button
              onClick={() => setCollapsed(true)}
              className="btn-tactile rounded p-1 text-text-muted hover:bg-ink-800 hover:text-text-strong"
              aria-label="Collapse controls"
              title="Collapse to button"
            >
              <Minimize2 className="h-3.5 w-3.5" />
            </button>
            <button
              onClick={() => getCurrentWindow().close()}
              className="btn-tactile rounded p-1 text-text-muted hover:bg-ink-800 hover:text-text-strong"
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
          showScroll
          sessionMode="display"
          displayMode={activeDisplayMode}
          surface="floating"
        />
      </div>
    </div>
  );
}
