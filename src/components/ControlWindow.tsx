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

    const tick = async () => {
      if (!active) return;
      try {
        const r = await mirrorRect(title);
        if (r && !r.minimized) {
          const w = (await win.outerSize()).width;
          let x = r.x + r.width; // dock to the right of the mirror
          if (x + w > r.workRight) x = r.x - w; // no room right -> dock left
          if (x < r.workLeft) x = r.workRight - w; // no room either -> overlay right edge
          await win.setPosition(new PhysicalPosition(x, r.y));
        }
      } catch {
        // mirror window may not be up yet, or moved off a monitor — ignore
      }
    };

    const id = setInterval(tick, 300);
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
