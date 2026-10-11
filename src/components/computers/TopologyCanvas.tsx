import { useState, useRef, useEffect } from "react";
import { Monitor, Move, MousePointer, ShieldCheck, AlertCircle, UploadCloud } from "lucide-react";
import { useKvmStore } from "@/stores/kvmStore";
import { getCurrentWebview } from "@tauri-apps/api/webview";

export function TopologyCanvas() {
  const topology = useKvmStore((s) => s.topology);
  const status = useKvmStore((s) => s.status);
  const settings = useKvmStore((s) => s.settings);
  const cursorPos = useKvmStore((s) => s.cursorPos);
  const setTopology = useKvmStore((s) => s.setTopology);
  const updateSettings = useKvmStore((s) => s.updateSettings);
  const sendFile = useKvmStore((s) => s.sendFile);

  const primary = topology.computers.find((c) => c.is_local) || topology.computers[0];
  const peer = topology.computers.find((c) => !c.is_local);

  const activeLink = topology.links[0];
  const [placement, setPlacement] = useState<"left" | "right">(() => {
    if (activeLink?.from_edge === "left" || activeLink?.from_edge === "right") {
      return activeLink.from_edge;
    }
    const saved = localStorage.getItem("anyleap_kvm_placement");
    return saved === "left" ? "left" : "right";
  });
  const [verticalOffset, setVerticalOffset] = useState<number>(activeLink?.offset_y || 0);

  const [isDragging, setIsDragging] = useState(false);
  const [isHoveringDrop, setIsHoveringDrop] = useState(false);
  const dragStartY = useRef(0);
  const startOffset = useRef(0);

  const isCursorRemote = status.is_input_trapped;
  const isCursorLocal = !status.is_input_trapped;

  // Listen for native OS drag and drop files onto the peer monitor card
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    try {
      const webview = getCurrentWebview();
      void webview.onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setIsHoveringDrop(true);
        } else if (event.payload.type === "drop") {
          setIsHoveringDrop(false);
          const paths = event.payload.paths;
          if (paths && paths.length > 0) {
            for (const path of paths) {
              void sendFile(path);
            }
          }
        } else {
          setIsHoveringDrop(false);
        }
      }).then((un) => {
        unlisten = un;
      });
    } catch {}

    return () => {
      if (unlisten) unlisten();
    };
  }, [sendFile]);

  const handleTogglePlacement = (side: "left" | "right") => {
    setPlacement(side);
    localStorage.setItem("anyleap_kvm_placement", side);
    updateTopologyLinks(side, verticalOffset);
  };

  const updateTopologyLinks = (side: "left" | "right", offsetY: number) => {
    if (primary && peer) {
      const nextTopology = {
        computers: [
          primary,
          {
            ...peer,
            bounds: {
              ...peer.bounds,
              x:
                side === "right"
                  ? primary.bounds.x + primary.bounds.width
                  : primary.bounds.x - peer.bounds.width,
              y: primary.bounds.y + offsetY,
            },
          },
        ],
        links: [
          {
            from_id: primary.id,
            from_edge: side,
            to_id: peer.id,
            to_edge: side === "right" ? ("left" as const) : ("right" as const),
            offset_y: offsetY,
            deadzone_px: settings.cornerDeadzonePx,
            resistance_ms: settings.edgeResistanceMs,
          },
        ],
      };
      void setTopology(nextTopology);
    }
  };

  const handleMouseDownDrag = (e: React.MouseEvent) => {
    setIsDragging(true);
    dragStartY.current = e.clientY;
    startOffset.current = verticalOffset;

    const handleMouseMove = (moveEvt: MouseEvent) => {
      const deltaY = moveEvt.clientY - dragStartY.current;
      const clamped = Math.max(-60, Math.min(60, startOffset.current + deltaY));
      setVerticalOffset(clamped);
    };

    const handleMouseUp = () => {
      setIsDragging(false);
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      updateTopologyLinks(placement, verticalOffset);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  const localMonitors =
    status.monitors && status.monitors.length > 0
      ? status.monitors
      : [
          {
            id: "mon-1",
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: true,
          },
        ];

  const deadzonePx = settings.cornerDeadzonePx;
  const deadzoneVisualH = Math.min(36, Math.max(12, Math.round((deadzonePx / 1080) * 160)));

  return (
    <div className="rounded-xl border border-[#1e2638] bg-[#090c14] p-5 space-y-4 select-none shadow-xl">
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-sky-400">
            Spatial Monitor Layout
          </span>
          {peer && (
            <span className="inline-flex items-center gap-1 rounded bg-emerald-500/10 px-2 py-0.5 font-mono text-[9px] font-semibold text-emerald-400 border border-emerald-500/20">
              <ShieldCheck className="h-3 w-3" />
              LINKED ACTIVE
            </span>
          )}
        </div>

        <div className="flex items-center gap-3">
          <span className="text-[11px] text-zinc-400">Peer screen side:</span>
          <div className="flex rounded-lg border border-[#1e2638] bg-[#0e1320] p-0.5">
            <button
              type="button"
              onClick={() => handleTogglePlacement("left")}
              className={`btn-tactile rounded px-3 py-1 text-[10px] font-mono cursor-pointer transition-colors ${
                placement === "left"
                  ? "bg-sky-600 text-white font-bold shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              Left
            </button>
            <button
              type="button"
              onClick={() => handleTogglePlacement("right")}
              className={`btn-tactile rounded px-3 py-1 text-[10px] font-mono cursor-pointer transition-colors ${
                placement === "right"
                  ? "bg-sky-600 text-white font-bold shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              Right
            </button>
          </div>
        </div>
      </div>

      {/* Blueprint Grid Canvas */}
      <div
        className={`relative flex h-72 w-full items-center justify-center rounded-xl border p-6 overflow-hidden shadow-inner transition-colors ${
          isHoveringDrop
            ? "border-emerald-500/60 bg-[#071318]"
            : "border-[#162034] bg-[#070a10]"
        }`}
        style={{
          backgroundImage: `
            linear-gradient(to right, rgba(30, 48, 77, 0.22) 1px, transparent 1px),
            linear-gradient(to bottom, rgba(30, 48, 77, 0.22) 1px, transparent 1px)
          `,
          backgroundSize: "24px 24px",
        }}
      >
        {isHoveringDrop && (
          <div className="absolute inset-0 z-20 flex flex-col items-center justify-center bg-black/60 backdrop-blur-xs text-emerald-400 space-y-2 pointer-events-none">
            <UploadCloud className="h-10 w-10 animate-bounce" />
            <span className="font-mono text-xs font-bold uppercase tracking-wider">
              Drop files to transfer to {peer?.name || "Peer PC"}
            </span>
            <span className="text-[10px] text-zinc-400">
              Saves to Downloads/AnyLeap_Transfers
            </span>
          </div>
        )}

        <div className="flex items-center gap-8 relative z-10">
          {/* Remote PC Card (when placed on LEFT) */}
          {placement === "left" && peer && (
            <div
              onMouseDown={handleMouseDownDrag}
              style={{ transform: `translateY(${verticalOffset}px)` }}
              className={`relative flex h-40 w-56 flex-col justify-between rounded-xl border-2 p-3 transition-shadow ${
                isDragging ? "cursor-grabbing opacity-90" : "cursor-grab"
              } ${
                isCursorRemote
                  ? "border-sky-400 bg-[#0e1b2f] shadow-[0_0_24px_rgba(56,189,248,0.35)]"
                  : "border-sky-500/40 bg-[#0c1424]/90 hover:border-sky-400/70"
              }`}
            >
              {isCursorRemote && (
                <div className="absolute -top-3 left-1/2 -translate-x-1/2 flex items-center gap-1 rounded-full bg-sky-500 px-2 py-0.5 text-[9px] font-mono font-bold text-zinc-950 shadow-md">
                  <MousePointer className="h-2.5 w-2.5 fill-current animate-pulse" />
                  <span>CURSOR HERE</span>
                </div>
              )}

              <div className="relative flex flex-1 items-center justify-center">
                <div className="relative flex h-20 w-32 flex-col items-center justify-center rounded-lg border border-sky-400/40 bg-[#16243a] shadow-inner overflow-hidden">
                  <Monitor className="h-5 w-5 text-sky-400" />
                  <span className="font-mono text-[9px] text-sky-200 mt-1">
                    {peer.bounds.width}x{peer.bounds.height}
                  </span>

                  {isCursorRemote && cursorPos && settings.indicatorCanvasDot && (
                    <div
                      className="absolute h-2.5 w-2.5 rounded-full bg-sky-300 ring-2 ring-white shadow-[0_0_8px_#38bdf8] transition-all duration-75 ease-out pointer-events-none"
                      style={{
                        left: `${(cursorPos.normX * 100).toFixed(1)}%`,
                        top: `${(cursorPos.normY * 100).toFixed(1)}%`,
                        transform: "translate(-50%, -50%)",
                      }}
                    />
                  )}
                </div>
              </div>

              <div className="text-center">
                <div className="font-bold text-xs text-sky-100 truncate uppercase tracking-wide">
                  {peer.name}
                </div>
                <div className="font-mono text-[9px] font-bold text-sky-400 tracking-wider">
                  LINKED (DROP FILES HERE)
                </div>
              </div>
            </div>
          )}

          {/* Primary PC ("This computer") Card */}
          <div
            className={`relative flex h-44 flex-col justify-between rounded-xl border-2 p-3 transition-shadow min-w-[260px] ${
              isCursorLocal
                ? "border-sky-400 bg-[#0e1b2f] shadow-[0_0_24px_rgba(56,189,248,0.25)]"
                : "border-sky-500/40 bg-[#0c1424]/90"
            }`}
          >
            {isCursorLocal && (
              <div className="absolute -top-3 left-1/2 -translate-x-1/2 flex items-center gap-1 rounded-full bg-sky-500 px-2 py-0.5 text-[9px] font-mono font-bold text-zinc-950 shadow-md">
                <MousePointer className="h-2.5 w-2.5 fill-current" />
                <span>CURSOR HERE</span>
              </div>
            )}

            {peer && (
              <div
                className={`absolute ${
                  placement === "right" ? "-right-3" : "-left-3"
                } top-4 bottom-4 w-1.5 flex flex-col justify-between pointer-events-none`}
              >
                <div
                  style={{ height: `${deadzoneVisualH}px` }}
                  className="w-full rounded-sm bg-amber-500/80 shadow-sm"
                  title={`Top deadzone: ${deadzonePx}px (Blocked)`}
                />
                <div className="flex-1 w-full bg-emerald-400 shadow-[0_0_6px_#34d399]" />
                <div
                  style={{ height: `${deadzoneVisualH}px` }}
                  className="w-full rounded-sm bg-amber-500/80 shadow-sm"
                  title={`Bottom deadzone: ${deadzonePx}px (Blocked)`}
                />
              </div>
            )}

            <div className="flex flex-1 items-end justify-center gap-2 py-1">
              {localMonitors.map((mon, idx) => {
                const isPortrait = mon.height > mon.width;
                const widthClass = isPortrait ? "w-14" : "w-24";
                const heightClass = isPortrait ? "h-22" : "h-18";

                return (
                  <div
                    key={mon.id || idx}
                    className={`flex ${widthClass} ${heightClass} flex-col items-center justify-center rounded-lg border border-sky-400/50 bg-[#17253d] relative overflow-hidden shadow-sm`}
                  >
                    <div className="absolute inset-0 bg-gradient-to-t from-sky-950/60 to-transparent pointer-events-none" />
                    <span className="font-mono text-[9px] font-semibold text-sky-200 relative z-10">
                      Display {idx + 1}
                    </span>
                    <span className="font-mono text-[8px] text-sky-400/80 relative z-10">
                      {mon.width}x{mon.height}
                    </span>
                  </div>
                );
              })}
            </div>

            <div className="text-center pt-1 border-t border-[#1a2b44]">
              <div className="font-bold text-xs text-sky-100 truncate">
                {status.local_name || "This computer"}
              </div>
              <div className="font-mono text-[9px] font-bold text-sky-400 tracking-wider">
                THIS COMPUTER
              </div>
            </div>
          </div>

          {/* Remote PC Card (when placed on RIGHT) */}
          {placement === "right" && peer && (
            <div
              onMouseDown={handleMouseDownDrag}
              style={{ transform: `translateY(${verticalOffset}px)` }}
              className={`relative flex h-40 w-56 flex-col justify-between rounded-xl border-2 p-3 transition-shadow ${
                isDragging ? "cursor-grabbing opacity-90" : "cursor-grab"
              } ${
                isCursorRemote
                  ? "border-sky-400 bg-[#0e1b2f] shadow-[0_0_24px_rgba(56,189,248,0.35)]"
                  : "border-sky-500/40 bg-[#0c1424]/90 hover:border-sky-400/70"
              }`}
            >
              {isCursorRemote && (
                <div className="absolute -top-3 left-1/2 -translate-x-1/2 flex items-center gap-1 rounded-full bg-sky-500 px-2 py-0.5 text-[9px] font-mono font-bold text-zinc-950 shadow-md">
                  <MousePointer className="h-2.5 w-2.5 fill-current animate-pulse" />
                  <span>CURSOR HERE</span>
                </div>
              )}

              <div className="relative flex flex-1 items-center justify-center">
                <div className="relative flex h-20 w-32 flex-col items-center justify-center rounded-lg border border-sky-400/40 bg-[#16243a] shadow-inner overflow-hidden">
                  <Monitor className="h-5 w-5 text-sky-400" />
                  <span className="font-mono text-[9px] text-sky-200 mt-1">
                    {peer.bounds.width}x{peer.bounds.height}
                  </span>

                  {isCursorRemote && cursorPos && settings.indicatorCanvasDot && (
                    <div
                      className="absolute h-2.5 w-2.5 rounded-full bg-sky-300 ring-2 ring-white shadow-[0_0_8px_#38bdf8] transition-all duration-75 ease-out pointer-events-none"
                      style={{
                        left: `${(cursorPos.normX * 100).toFixed(1)}%`,
                        top: `${(cursorPos.normY * 100).toFixed(1)}%`,
                        transform: "translate(-50%, -50%)",
                      }}
                    />
                  )}
                </div>
              </div>

              <div className="text-center">
                <div className="font-bold text-xs text-sky-100 truncate uppercase tracking-wide">
                  {peer.name}
                </div>
                <div className="font-mono text-[9px] font-bold text-sky-400 tracking-wider">
                  LINKED (DROP FILES HERE)
                </div>
              </div>
            </div>
          )}

          {!peer && (
            <div className="flex h-40 w-56 flex-col items-center justify-center rounded-xl border border-dashed border-[#1f2d44] bg-[#0b101c]/60 p-3 text-center">
              <span className="text-xs text-zinc-400 font-medium">No Peer Connected</span>
              <span className="text-[10px] text-zinc-500 mt-1 max-w-[140px]">
                Select a discovered computer below or enter IP to link.
              </span>
            </div>
          )}
        </div>

        {peer && (
          <div className="absolute bottom-2 right-3 flex items-center gap-1 text-[10px] font-mono text-zinc-500">
            <Move className="h-3 w-3" />
            <span>Drag card vertically to align boundaries</span>
          </div>
        )}
      </div>

      {peer && (
        <div className="flex items-center justify-between rounded-xl border border-[#1e2638] bg-[#0e1320] px-4 py-2.5 text-xs">
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-1.5 text-zinc-300">
              <span className="h-2 w-2 rounded-full bg-emerald-400" />
              <span className="font-mono text-[11px]">Active Gateway</span>
            </div>
            <div className="flex items-center gap-1.5 text-zinc-300">
              <span className="h-2 w-2 rounded-sm bg-amber-400" />
              <span className="font-mono text-[11px]">
                Corner Deadzone ({deadzonePx}px)
              </span>
            </div>
            <div className="flex items-center gap-1.5 text-zinc-400">
              <AlertCircle className="h-3 w-3 text-sky-400" />
              <span className="font-mono text-[11px]">
                Push Dwell: {settings.edgeResistanceMs}ms
              </span>
            </div>
          </div>

          <div className="flex items-center gap-3">
            <span className="text-[10px] text-zinc-500">Quick Deadzone:</span>
            {[24, 48, 80].map((val) => (
              <button
                key={val}
                type="button"
                onClick={() => updateSettings({ cornerDeadzonePx: val })}
                className={`rounded px-2 py-0.5 font-mono text-[10px] cursor-pointer ${
                  deadzonePx === val
                    ? "bg-sky-500 text-zinc-950 font-bold"
                    : "bg-zinc-800 text-zinc-400 hover:text-white"
                }`}
              >
                {val}px
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
