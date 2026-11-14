import { useState, useRef, useEffect, useMemo } from "react";
import {
  Move,
  MousePointer,
  ShieldCheck,
  AlertCircle,
  UploadCloud,
} from "lucide-react";
import type { EdgeSide, TopologyConfig } from "@/types/kvm";
import { useKvmStore } from "@/stores/kvmStore";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  clampEdgeOffset,
  computeCanvasLayout,
  computePeerOrigin,
  oppositeEdge,
} from "./topologyLayout";

const EDGE_OPTIONS: { side: EdgeSide; label: string }[] = [
  { side: "left", label: "Left" },
  { side: "right", label: "Right" },
  { side: "top", label: "Top" },
  { side: "bottom", label: "Bottom" },
];

function getLinkEdge(edge?: string | null): EdgeSide {
  if (edge === "left" || edge === "right" || edge === "top" || edge === "bottom") {
    return edge;
  }
  return "right";
}

export function TopologyCanvas() {
  const topology = useKvmStore((s) => s.topology);
  const status = useKvmStore((s) => s.status);
  const settings = useKvmStore((s) => s.settings);
  const cursorPos = useKvmStore((s) => s.cursorPos);
  const setTopology = useKvmStore((s) => s.setTopology);
  const updateSettings = useKvmStore((s) => s.updateSettings);
  const sendFile = useKvmStore((s) => s.sendFile);

  const primary =
    topology.computers.find((c) => c.is_local) || topology.computers[0];
  const peer = topology.computers.find((c) => !c.is_local) || null;
  const activeLink = topology.links[0];

  const [placement, setPlacement] = useState<EdgeSide>(() => {
    if (activeLink?.from_edge) return getLinkEdge(activeLink.from_edge);
    if (status.controller_edge) return getLinkEdge(status.controller_edge);
    const saved = localStorage.getItem("anyleap_kvm_placement");
    return getLinkEdge(saved);
  });

  const [edgeOffset, setEdgeOffset] = useState<number>(() => {
    if (!activeLink) return 0;
    return activeLink.from_edge === "top" || activeLink.from_edge === "bottom"
      ? activeLink.offset_x ?? 0
      : activeLink.offset_y ?? 0;
  });

  const [isDragging, setIsDragging] = useState(false);
  const [isHoveringDrop, setIsHoveringDrop] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 760, height: 288 });
  const latestOffsetRef = useRef(edgeOffset);
  latestOffsetRef.current = edgeOffset;

  // Measure canvas container for accurate to-scale rendering
  useEffect(() => {
    const el = containerRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const update = () => {
      const rect = el.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        setCanvasSize({ width: rect.width, height: rect.height });
      }
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Sync placement and offset from active link on reconnect / status refresh (M2)
  useEffect(() => {
    if (isDragging) return;
    const linkEdge = activeLink?.from_edge
      ? getLinkEdge(activeLink.from_edge)
      : status.controller_edge
        ? getLinkEdge(status.controller_edge)
        : null;
    if (linkEdge) {
      setPlacement(linkEdge);
      const nextOffset =
        linkEdge === "top" || linkEdge === "bottom"
          ? activeLink?.offset_x ?? 0
          : activeLink?.offset_y ?? 0;
      setEdgeOffset(nextOffset);
      latestOffsetRef.current = nextOffset;
    }
  }, [
    activeLink?.from_edge,
    activeLink?.offset_x,
    activeLink?.offset_y,
    status.controller_edge,
    isDragging,
  ]);

  // Listen for native OS drag and drop files onto the canvas
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    try {
      const webview = getCurrentWebview();
      void webview
        .onDragDropEvent((event) => {
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
        })
        .then((un) => {
          unlisten = un;
        });
    } catch {
      // non-Tauri test environment
    }

    return () => {
      if (unlisten) unlisten();
    };
  }, [sendFile]);

  const localNodeForLayout = useMemo(
    () => ({
      bounds: primary?.bounds ?? { x: 0, y: 0, width: 1920, height: 1080 },
      monitors:
        primary?.monitors && primary.monitors.length > 0
          ? primary.monitors
          : status.monitors ?? [],
    }),
    [primary, status.monitors]
  );

  const layout = useMemo(
    () =>
      computeCanvasLayout({
        localNode: localNodeForLayout,
        peerNode: peer,
        edge: placement,
        offsetPx: edgeOffset,
        canvasWidth: canvasSize.width,
        canvasHeight: canvasSize.height,
        padding: 32,
      }),
    [localNodeForLayout, peer, placement, edgeOffset, canvasSize.width, canvasSize.height]
  );

  const submitTopology = (side: EdgeSide, offsetPx: number) => {
    if (!primary || !peer) return;
    const isTopBottom = side === "top" || side === "bottom";
    const peerOrigin = computePeerOrigin(
      layout.localUnionPhysical,
      layout.peerUnionPhysical,
      side,
      offsetPx
    );
    const nextTopology: TopologyConfig = {
      computers: [
        primary,
        {
          ...peer,
          bounds: {
            x: peerOrigin.x,
            y: peerOrigin.y,
            width: layout.peerUnionPhysical.width,
            height: layout.peerUnionPhysical.height,
          },
        },
      ],
      links: [
        {
          from_id: primary.id,
          from_edge: side,
          to_id: peer.id,
          to_edge: oppositeEdge(side),
          offset_x: isTopBottom ? offsetPx : 0,
          offset_y: isTopBottom ? 0 : offsetPx,
          deadzone_px: settings.cornerDeadzonePx,
          resistance_ms: settings.edgeResistanceMs,
        },
      ],
    };
    void setTopology(nextTopology);
  };

  const handleTogglePlacement = (side: EdgeSide) => {
    const clamped = clampEdgeOffset(
      layout.localUnionPhysical,
      layout.peerUnionPhysical,
      side,
      0
    );
    setPlacement(side);
    setEdgeOffset(clamped);
    latestOffsetRef.current = clamped;
    localStorage.setItem("anyleap_kvm_placement", side);
    submitTopology(side, clamped);
  };

  const handleMouseDownDrag = (e: React.MouseEvent) => {
    if (!peer) return;
    e.preventDefault();
    setIsDragging(true);
    const startClientX = e.clientX;
    const startClientY = e.clientY;
    const startOffset = edgeOffset;
    const scale = Math.max(0.01, layout.scale);
    const isTopBottom = placement === "top" || placement === "bottom";

    const handleMouseMove = (moveEvt: MouseEvent) => {
      const deltaCanvasPx = isTopBottom
        ? moveEvt.clientX - startClientX
        : moveEvt.clientY - startClientY;
      const deltaPhysical = Math.round(deltaCanvasPx / scale);
      const clamped = clampEdgeOffset(
        layout.localUnionPhysical,
        layout.peerUnionPhysical,
        placement,
        startOffset + deltaPhysical
      );
      latestOffsetRef.current = clamped;
      setEdgeOffset(clamped);
    };

    const handleMouseUp = () => {
      setIsDragging(false);
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      submitTopology(placement, latestOffsetRef.current);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  const isCursorRemote = status.is_input_trapped;
  const isCursorLocal = !status.is_input_trapped;
  const deadzonePx = settings.cornerDeadzonePx;

  const isHorizontalSharedEdge = placement === "left" || placement === "right";
  const deadzoneCanvasPx =
    deadzonePx <= 0
      ? 0
      : Math.min(24, Math.max(4, Math.round(deadzonePx * layout.scale)));

  return (
    <div className="rounded-xl border border-ink-700 bg-ink-900 p-5 space-y-4 select-none">
      <div className="flex flex-wrap items-center justify-between gap-3 text-xs">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-gold-400">
            Screen Layout (To Scale)
          </span>
          {peer && (
            <span className="inline-flex items-center gap-1 rounded bg-status-success-bg px-2 py-0.5 font-mono text-[9px] font-semibold text-status-success border border-status-success/40">
              <ShieldCheck className="h-3 w-3" />
              LINKED ACTIVE
            </span>
          )}
        </div>

        <div className="flex items-center gap-2.5">
          <span className="text-[11px] text-text-muted">Linked PC edge:</span>
          <div className="flex rounded-lg border border-ink-700 bg-ink-950 p-0.5">
            {EDGE_OPTIONS.map(({ side, label }) => (
              <button
                key={side}
                type="button"
                onClick={() => handleTogglePlacement(side)}
                className={`btn-tactile rounded px-2.5 py-1 text-[10px] font-mono cursor-pointer ${
                  placement === side
                    ? "bg-gold-400 text-ink-950 font-bold"
                    : "text-text-muted hover:text-text-strong"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Scaled Blueprint Grid Canvas */}
      <div
        ref={containerRef}
        className={`relative h-72 w-full rounded-xl border overflow-hidden transition-colors duration-150 ${
          isHoveringDrop
            ? "border-status-success bg-status-success-bg/40"
            : "border-ink-700 bg-ink-950"
        }`}
      >
        <svg className="pointer-events-none absolute inset-0 h-full w-full" aria-hidden="true">
          <defs>
            <pattern id="topology-grid" width="24" height="24" patternUnits="userSpaceOnUse">
              <path d="M 24 0 L 0 0 0 24" fill="none" stroke="var(--ink-800)" strokeWidth="1" />
            </pattern>
          </defs>
          <rect width="100%" height="100%" fill="url(#topology-grid)" />
        </svg>

        {isHoveringDrop && (
          <div className="absolute inset-0 z-20 flex flex-col items-center justify-center bg-ink-950/90 text-status-success space-y-2 pointer-events-none">
            <UploadCloud className="h-10 w-10" />
            <span className="font-mono text-xs font-bold uppercase tracking-wider">
              Drop files to transfer to {peer?.name || "Linked PC"}
            </span>
            <span className="text-[10px] text-text-muted">
              Saves to Downloads/AnyLeap_Transfers
            </span>
          </div>
        )}

        {/* Main PC Union Outline + Monitors */}
        <div
          style={{
            left: `${layout.localUnionCanvas.left}px`,
            top: `${layout.localUnionCanvas.top}px`,
            width: `${layout.localUnionCanvas.width}px`,
            height: `${layout.localUnionCanvas.height}px`,
          }}
          className={`absolute rounded-lg border transition-colors duration-150 ${
            isCursorLocal
              ? "border-mist-300"
              : "border-ink-700"
          }`}
        >
          <div className="absolute -top-5 left-0 flex items-center gap-1.5 whitespace-nowrap">
            <span className="font-mono text-[10px] font-bold text-mist-300">
              {status.local_name || "Main PC"}
            </span>
            {isCursorLocal && (
              <span className="inline-flex items-center gap-0.5 rounded bg-mist-300 px-1.5 py-0.2 text-[8px] font-mono font-bold text-ink-950">
                <MousePointer className="h-2 w-2 fill-current" />
                CURSOR
              </span>
            )}
          </div>
        </div>

        {/* Shared Edge Gateway + Corner Deadzones (only along overlapping edge-monitor spans) */}
        {peer &&
          layout.sharedEdgeSpans.map((span, idx) => {
            const spanLen = isHorizontalSharedEdge
              ? span.canvas.height
              : span.canvas.width;
            const dzPx =
              deadzoneCanvasPx <= 0
                ? 0
                : Math.min(spanLen / 2, deadzoneCanvasPx);
            return (
              <div
                key={`${span.physicalStart}-${span.physicalEnd}-${idx}`}
                style={{
                  left: `${span.canvas.left}px`,
                  top: `${span.canvas.top}px`,
                  width: `${span.canvas.width}px`,
                  height: `${span.canvas.height}px`,
                }}
                className={`absolute flex ${
                  isHorizontalSharedEdge ? "flex-col" : "flex-row"
                } justify-between pointer-events-none z-10`}
              >
                {dzPx > 0 && (
                  <div
                    style={
                      isHorizontalSharedEdge
                        ? { height: `${dzPx}px` }
                        : { width: `${dzPx}px` }
                    }
                    className={`${
                      isHorizontalSharedEdge ? "w-full" : "h-full"
                    } rounded-sm bg-status-warning/85 shrink-0`}
                  />
                )}
                <div className="flex-1 bg-gold-400" />
                {dzPx > 0 && (
                  <div
                    style={
                      isHorizontalSharedEdge
                        ? { height: `${dzPx}px` }
                        : { width: `${dzPx}px` }
                    }
                    className={`${
                      isHorizontalSharedEdge ? "w-full" : "h-full"
                    } rounded-sm bg-status-warning/85 shrink-0`}
                  />
                )}
              </div>
            );
          })}

        {/* Individual Main PC Monitors */}
        {layout.localMonitors.map((mon, idx) => (
          <div
            key={mon.id || idx}
            style={{
              left: `${mon.canvas.left}px`,
              top: `${mon.canvas.top}px`,
              width: `${mon.canvas.width}px`,
              height: `${mon.canvas.height}px`,
            }}
            className="absolute flex flex-col items-center justify-center rounded border border-mist-300/50 bg-ink-800 overflow-hidden pointer-events-none"
          >
            <span className="font-mono text-[9px] font-semibold text-text-strong truncate max-w-full px-1">
              {mon.isPrimary ? "Main" : `D${idx + 1}`}
            </span>
            <span className="font-mono text-[8px] text-mist-300 truncate max-w-full px-1">
              {mon.physical.width}×{mon.physical.height}
            </span>
          </div>
        ))}

        {/* Linked PC Union (Draggable) + Monitors */}
        {peer && (
          <div
            onMouseDown={handleMouseDownDrag}
            style={{
              left: `${layout.peerUnionCanvas.left}px`,
              top: `${layout.peerUnionCanvas.top}px`,
              width: `${layout.peerUnionCanvas.width}px`,
              height: `${layout.peerUnionCanvas.height}px`,
            }}
            className={`absolute rounded-lg border transition-colors duration-150 ${
              isDragging ? "cursor-grabbing opacity-90" : "cursor-grab"
            } ${
              isCursorRemote
                ? "border-gold-400"
                : "border-status-success/60 hover:border-status-success"
            }`}
          >
            <div className="absolute -bottom-5 left-0 flex items-center gap-1.5 whitespace-nowrap">
              <span className="font-mono text-[10px] font-bold text-status-success">
                {peer.name}
              </span>
              {isCursorRemote && (
                <span className="inline-flex items-center gap-0.5 rounded bg-gold-400 px-1.5 py-0.2 text-[8px] font-mono font-bold text-ink-950">
                  <MousePointer className="h-2 w-2 fill-current" />
                  CURSOR
                </span>
              )}
            </div>

            {isCursorRemote && cursorPos && settings.indicatorCanvasDot && (
              <div
                className="absolute z-20 h-2.5 w-2.5 rounded-full bg-gold-400 ring-2 ring-text-strong transition-[left,top] duration-75 ease-out pointer-events-none"
                style={{
                  left: `${(cursorPos.normX * 100).toFixed(1)}%`,
                  top: `${(cursorPos.normY * 100).toFixed(1)}%`,
                  transform: "translate(-50%, -50%)",
                }}
              />
            )}
          </div>
        )}

        {/* Individual Linked PC Monitors */}
        {peer &&
          layout.peerMonitors.map((mon, idx) => (
            <div
              key={mon.id || idx}
              style={{
                left: `${mon.canvas.left}px`,
                top: `${mon.canvas.top}px`,
                width: `${mon.canvas.width}px`,
                height: `${mon.canvas.height}px`,
              }}
              className="absolute flex flex-col items-center justify-center rounded border border-status-success/50 bg-status-success-bg overflow-hidden pointer-events-none"
            >
              <span className="font-mono text-[9px] font-semibold text-text-strong truncate max-w-full px-1">
                {mon.isPrimary ? "Primary" : `D${idx + 1}`}
              </span>
              <span className="font-mono text-[8px] text-status-success truncate max-w-full px-1">
                {mon.physical.width}×{mon.physical.height}
              </span>
            </div>
          ))}

        {peer && (
          <div className="absolute bottom-2 right-3 flex items-center gap-1.5 text-[10px] font-mono text-text-muted pointer-events-none">
            <Move className="h-3 w-3" />
            <span>
              Drag {peer.name} along edge (offset: {edgeOffset}px)
            </span>
          </div>
        )}
      </div>

      {/* Deadzone & Dwell Controls */}
      {peer && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-ink-700 bg-ink-950 px-4 py-2.5 text-xs">
          <div className="flex flex-wrap items-center gap-4">
            <div className="flex items-center gap-1.5 text-text">
              <span className="h-2 w-2 rounded-full bg-gold-400" />
              <span className="font-mono text-[11px]">Active Edge</span>
            </div>
            <div className="flex items-center gap-1.5 text-text">
              <span className="h-2 w-2 rounded-sm bg-status-warning" />
              <span className="font-mono text-[11px]">
                Deadzone: {deadzonePx}px
              </span>
              <div className="flex items-center gap-1 ml-1">
                {[24, 48, 80].map((val) => (
                  <button
                    key={val}
                    type="button"
                    onClick={() => updateSettings({ cornerDeadzonePx: val })}
                    className={`btn-tactile rounded px-2 py-0.5 font-mono text-[10px] cursor-pointer ${
                      deadzonePx === val
                        ? "bg-gold-400 text-ink-950 font-bold"
                        : "bg-ink-800 text-text-muted hover:text-text-strong"
                    }`}
                  >
                    {val}px
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <AlertCircle className="h-3 w-3 text-mist-300" />
            <span className="font-mono text-[11px] text-text">
              Dwell: {settings.edgeResistanceMs}ms
            </span>
            <div className="flex items-center gap-1">
              {[0, 80, 120, 200].map((ms) => (
                <button
                  key={ms}
                  type="button"
                  onClick={() => updateSettings({ edgeResistanceMs: ms })}
                  className={`btn-tactile rounded px-2 py-0.5 font-mono text-[10px] cursor-pointer ${
                    settings.edgeResistanceMs === ms
                      ? "bg-gold-400 text-ink-950 font-bold"
                      : "bg-ink-800 text-text-muted hover:text-text-strong"
                  }`}
                >
                  {ms}ms
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
