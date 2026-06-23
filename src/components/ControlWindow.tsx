import { getCurrentWindow } from "@tauri-apps/api/window";
import { GripHorizontal, X } from "lucide-react";
import { ControlBar } from "@/components/ControlBar";

/** Root of the floating, always-on-top control window (separate webview). */
export function ControlWindow({ serial }: { serial: string }) {
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
