import { useEffect, useRef, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Minus, Square, X, GripHorizontal } from "lucide-react";
import {
  embedMirror,
  resizeEmbeddedMirror,
  pushClipboardImage,
  stopMirror,
  onSessionExited,
} from "@/lib/tauri";
import { ControlBar } from "./ControlBar";
import { DEFAULT_CONTROL_CONFIG, loadControlConfig, type ControlConfig } from "@/lib/controlConfig";
import { useAppStore } from "@/store/useAppStore";

export function EmbeddedMirrorWindow({ serial }: { serial: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [config, setConfig] = useState<ControlConfig>(DEFAULT_CONTROL_CONFIG);
  const [toast, setToast] = useState<string | null>(null);
  const win = getCurrentWebviewWindow();
  const sessions = useAppStore((s) => s.sessions);
  const session = sessions.find((s) => s.serial === serial);

  useEffect(() => {
    loadControlConfig().then(setConfig).catch(() => {});
  }, []);
  useEffect(() => {
    const unlistenPromise = onSessionExited((e) => {
      const activeSessions = useAppStore.getState().sessions;
      const current = activeSessions.find((s) => s.id === e.payload.id);
      if (
        current?.serial === serial ||
        !activeSessions.some((s) => s.serial === serial && s.id !== e.payload.id)
      ) {
        win.close().catch(() => {});
      }
    });

    const unlistenClosePromise = win.onCloseRequested(async () => {
      const active = useAppStore.getState().sessions.find((s) => s.serial === serial);
      if (active) {
        await stopMirror(active.id).catch(() => {});
      }
    });

    return () => {
      unlistenPromise.then((un) => un()).catch(() => {});
      unlistenClosePromise.then((un) => un()).catch(() => {});
    };
  }, [serial, win]);

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

  useEffect(() => {
    const handlePaste = async (e: ClipboardEvent) => {
      const items = e.clipboardData?.items;
      if (!items) return;
      for (const item of Array.from(items)) {
        if (item.type.startsWith("image/")) {
          const file = item.getAsFile();
          if (!file) continue;
          try {
            const buf = await file.arrayBuffer();
            const bytes = Array.from(new Uint8Array(buf));
            const dest = await pushClipboardImage(serial, bytes);
            setToast(`Image pushed to ${dest}`);
            setTimeout(() => setToast(null), 3000);
          } catch (err) {
            setToast(`Paste failed: ${err}`);
            setTimeout(() => setToast(null), 4000);
          }
          break;
        }
      }
    };

    window.addEventListener("paste", handlePaste);
    return () => window.removeEventListener("paste", handlePaste);
  }, [serial]);

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-zinc-950 text-zinc-100 select-none">
      {/* Custom Title Bar */}
      <div
        onPointerDown={(e) => {
          if (e.button === 0) {
            win.startDragging().catch(() => {});
          }
        }}
        onDoubleClick={() => {
          win.toggleMaximize().catch(() => {});
        }}
        className="flex h-9 cursor-grab items-center justify-between border-b border-zinc-800/80 bg-zinc-900 px-3 shrink-0 select-none active:cursor-grabbing"
      >
        <div className="flex items-center gap-2 pointer-events-none">
          <GripHorizontal className="h-3.5 w-3.5 text-zinc-500" />
          <span className="text-xs font-medium text-zinc-200">AnyLeap — {serial}</span>
        </div>
        <div
          className="flex items-center gap-1"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => e.stopPropagation()}
        >
          <button
            type="button"
            onClick={() => win.minimize().catch(console.error)}
            className="rounded p-1 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200 transition-colors"
            aria-label="Minimize"
          >
            <Minus className="h-3 w-3" />
          </button>
          <button
            type="button"
            onClick={() => win.toggleMaximize().catch(console.error)}
            className="rounded p-1 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200 transition-colors"
            aria-label="Maximize"
          >
            <Square className="h-3 w-3" />
          </button>
          <button
            type="button"
            onClick={() => {
              if (session) {
                stopMirror(session.id).catch(() => {});
              }
              win.close().catch(console.error);
            }}
            className="rounded p-1 text-zinc-400 hover:bg-red-900/50 hover:text-red-300 transition-colors"
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
            sessionMode={session?.mode}
          />
        </div>
      </div>
      {toast && (
        <div className="absolute bottom-4 left-1/2 -translate-x-1/2 z-50 rounded-lg bg-zinc-800/95 border border-cyan-500/40 px-3 py-1.5 text-xs text-cyan-300 shadow-xl backdrop-blur-md transition-all">
          {toast}
        </div>
      )}
    </div>
  );
}
