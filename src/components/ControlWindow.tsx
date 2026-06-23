import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { PhysicalPosition } from "@tauri-apps/api/dpi";
import { GripHorizontal, X } from "lucide-react";
import { ControlBar } from "@/components/ControlBar";
import { mirrorRect } from "@/lib/tauri";

/** Root of the floating, always-on-top control window (separate webview).
 *  Docks itself to the scrcpy mirror window and follows it. */
export function ControlWindow({ serial }: { serial: string }) {
  useEffect(() => {
    const title = `AnyLeap — ${serial}`;
    const win = getCurrentWindow();
    let active = true;
    let visible = true;
    let lastX = Number.NaN;
    let lastY = Number.NaN;
    let stripW = 88;
    win.outerSize().then((s) => (stripW = s.width)).catch(() => {});

    const tick = async () => {
      if (!active) return;
      try {
        const r = await mirrorRect(title);
        if (!r || r.minimized) {
          // Mirror minimized / gone -> hide the strip until it returns.
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
        let x = r.x + r.width; // dock to the right of the mirror
        if (x + stripW > r.workRight) x = r.x - stripW; // no room right -> dock left
        if (x < r.workLeft) x = r.workRight - stripW; // no room either -> right edge
        if (x !== lastX || r.y !== lastY) {
          lastX = x;
          lastY = r.y;
          await win.setPosition(new PhysicalPosition(x, r.y));
        }
      } catch {
        // mirror window may not be up yet, or moved off a monitor — ignore
      }
    };

    const id = setInterval(tick, 50);
    void tick();
    return () => {
      active = false;
      clearInterval(id);
    };
  }, [serial]);

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
        <ControlBar serial={serial} orientation="vertical" />
      </div>
    </div>
  );
}
