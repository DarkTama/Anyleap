import { useState, useRef, useEffect, useMemo } from "react";
import {
  Move,
  MousePointer,
  ShieldCheck,
  AlertCircle,
  AlertTriangle,
  UploadCloud,
  X,
} from "lucide-react";
import type { EdgeSide, Point, Seam, TopologyConfig } from "@/types/kvm";
import { useKvmStore } from "@/stores/kvmStore";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  computeCanvasLayout,
  computePresetOrigin,
  hasMonitorOverlap,
  normalizeMonitors,
  oppositeEdge,
  primarySeamSide,
  resolveRemoteOrigin,
  snapRemoteOrigin,
  type CanvasViewport,
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

function shortMonitorLabel(
  deviceName: string | undefined,
  isPrimary: boolean,
  idx: number
): string {
  if (isPrimary) return "Main";
  if (deviceName) {
    const match = deviceName.match(/DISPLAY(\d+)/i);
    if (match) return `D${match[1]}`;
  }
  return `D${idx + 1}`;
}

export function TopologyCanvas() {
  const topology = useKvmStore((s) => s.topology);
  const status = useKvmStore((s) => s.status);
  const settings = useKvmStore((s) => s.settings);
  const cursorPos = useKvmStore((s) => s.cursorPos);
  const monitorChangeNotice = useKvmStore((s) => s.monitorChangeNotice);
  const dismissMonitorChangeNotice = useKvmStore((s) => s.dismissMonitorChangeNotice);
  const setTopology = useKvmStore((s) => s.setTopology);
  const updateSettings = useKvmStore((s) => s.updateSettings);
  const sendFile = useKvmStore((s) => s.sendFile);

  const primary =
    topology.computers.find((c) => c.is_local) || topology.computers[0];
  const peer = topology.computers.find((c) => !c.is_local) || null;
  const activeLink = topology.links[0];

  const mainMonitors = useMemo(
    () =>
      normalizeMonitors(
        primary?.monitors && primary.monitors.length > 0
          ? primary.monitors
          : status.monitors,
        primary?.bounds ?? { x: 0, y: 0, width: 1920, height: 1080 },
        "local"
      ),
    [primary, status.monitors]
  );

  const peerMonitors = useMemo(
    () =>
      peer
        ? normalizeMonitors(peer.monitors, peer.bounds, "peer")
        : [],
    [peer]
  );

  const fallbackSide = useMemo<EdgeSide>(() => {
    if (activeLink?.from_edge) return getLinkEdge(activeLink.from_edge);
    if (status.controller_edge) return getLinkEdge(status.controller_edge);
    if (typeof localStorage !== "undefined") {
      return getLinkEdge(localStorage.getItem("anyleap_kvm_placement"));
    }
    return "right";
  }, [activeLink?.from_edge, status.controller_edge]);

  const resolvedSavedOrigin = useMemo<Point>(
    () =>
      peer
        ? resolveRemoteOrigin({
            mainMonitors,
            linkedMonitors: peerMonitors,
            remoteOrigin: topology.remote_origin ?? status.remote_origin ?? null,
            link: activeLink ?? null,
            fallbackSide,
          })
        : { x: 0, y: 0 },
    [
      peer,
      mainMonitors,
      peerMonitors,
      topology.remote_origin,
      status.remote_origin,
      activeLink,
      fallbackSide,
    ]
  );

  const [draftOrigin, setDraftOrigin] = useState<Point>(resolvedSavedOrigin);
  const [isDragging, setIsDragging] = useState(false);
  const [dragState, setDragState] = useState<{
    overlaps: boolean;
    hasSeam: boolean;
  }>({ overlaps: false, hasSeam: true });
  const [isHoveringDrop, setIsHoveringDrop] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);
  const [canvasSize, setCanvasSize] = useState({ width: 760, height: 320 });

  const activeDragRef = useRef<{
    pointerId: number;
    startClientX: number;
    startClientY: number;
    startOrigin: Point;
    lastValidOrigin: Point;
    viewport: CanvasViewport;
    latestOrigin: Point;
    latestValid: boolean;
    latestSeams: Seam[];
  } | null>(null);

  // Sync local draftOrigin with saved topology when not dragging
  useEffect(() => {
    if (isDragging) return;
    setDraftOrigin(resolvedSavedOrigin);
  }, [resolvedSavedOrigin, isDragging]);

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
      monitors: mainMonitors,
    }),
    [primary?.bounds, mainMonitors]
  );

  const peerNodeForLayout = useMemo(
    () =>
      peer
        ? {
            bounds: peer.bounds,
            monitors: peerMonitors,
          }
        : null,
    [peer, peerMonitors]
  );

  const layout = useMemo(
    () =>
      computeCanvasLayout({
        localNode: localNodeForLayout,
        peerNode: peerNodeForLayout,
        remoteOrigin: draftOrigin,
        canvasWidth: canvasSize.width,
        canvasHeight: canvasSize.height,
        padding: 36,
        frozenViewport: isDragging ? activeDragRef.current?.viewport ?? null : null,
      }),
    [
      localNodeForLayout,
      peerNodeForLayout,
      draftOrigin,
      canvasSize.width,
      canvasSize.height,
      isDragging,
    ]
  );

  const submitTopology = (origin: Point, primarySide: EdgeSide) => {
    if (!primary || !peer) return;
    const roundedOrigin: Point = {
      x: Math.round(origin.x),
      y: Math.round(origin.y),
    };
    const nextLayout = computeCanvasLayout({
      localNode: localNodeForLayout,
      peerNode: peerNodeForLayout,
      remoteOrigin: roundedOrigin,
      canvasWidth: canvasSize.width,
      canvasHeight: canvasSize.height,
      padding: 36,
    });
    const nextTopology: TopologyConfig = {
      computers: [
        primary,
        {
          ...peer,
          bounds: {
            x: nextLayout.peerUnionPhysical.x,
            y: nextLayout.peerUnionPhysical.y,
            width: nextLayout.peerUnionPhysical.width,
            height: nextLayout.peerUnionPhysical.height,
          },
        },
      ],
      links: [
        {
          from_id: primary.id,
          from_edge: primarySide,
          to_id: peer.id,
          to_edge: oppositeEdge(primarySide),
          offset_x: 0,
          offset_y: 0,
          deadzone_px: settings.cornerDeadzonePx,
          resistance_ms: settings.edgeResistanceMs,
        },
      ],
      remote_origin: roundedOrigin,
    };
    void setTopology(nextTopology);
  };

  const handlePresetSide = (side: EdgeSide) => {
    if (!peer) return;
    const presetOrigin = computePresetOrigin(mainMonitors, peerMonitors, side);
    setDraftOrigin(presetOrigin);
    if (typeof localStorage !== "undefined") {
      localStorage.setItem("anyleap_kvm_placement", side);
    }
    submitTopology(presetOrigin, side);
  };

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!peer) return;
    if (e.button !== 0 && e.pointerType === "mouse") return;
    if (activeDragRef.current) return; // multi-touch protection

    e.preventDefault();
    try {
      e.currentTarget.setPointerCapture?.(e.pointerId);
    } catch {
      // jsdom / synthetic event fallback
    }

    const startOrigin = draftOrigin;
    const initialValid =
      !hasMonitorOverlap(mainMonitors, peerMonitors, startOrigin) &&
      layout.seams.length > 0;

    activeDragRef.current = {
      pointerId: e.pointerId,
      startClientX: e.clientX,
      startClientY: e.clientY,
      startOrigin,
      lastValidOrigin: initialValid ? startOrigin : resolvedSavedOrigin,
      viewport: layout.viewport,
      latestOrigin: startOrigin,
      latestValid: initialValid,
      latestSeams: layout.seams,
    };
    setDragState({ overlaps: false, hasSeam: layout.seams.length > 0 });
    setIsDragging(true);
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = activeDragRef.current;
    if (!drag || drag.pointerId !== e.pointerId) return;

    const scale = Math.max(1e-6, drag.viewport.scale);
    const dxPhys = Math.round((e.clientX - drag.startClientX) / scale);
    const dyPhys = Math.round((e.clientY - drag.startClientY) / scale);
    const candidateOrigin: Point = {
      x: drag.startOrigin.x + dxPhys,
      y: drag.startOrigin.y + dyPhys,
    };

    const snap = snapRemoteOrigin({
      mainMonitors,
      linkedMonitors: peerMonitors,
      candidateOrigin,
      scale,
      maxSnapScreenPx: 24,
    });

    drag.latestOrigin = snap.origin;
    drag.latestValid = snap.valid;
    drag.latestSeams = snap.seams;
    setDragState({
      overlaps: snap.overlaps,
      hasSeam: snap.seams.length > 0,
    });
    setDraftOrigin(snap.origin);
  };

  const finishDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = activeDragRef.current;
    if (!drag || drag.pointerId !== e.pointerId) return;

    try {
      e.currentTarget.releasePointerCapture?.(e.pointerId);
    } catch {
      // ignore if not captured
    }
    activeDragRef.current = null;
    setIsDragging(false);
    setDragState({ overlaps: false, hasSeam: true });

    if (drag.latestValid && drag.latestSeams.length > 0) {
      const side = primarySeamSide(drag.latestSeams, fallbackSide);
      setDraftOrigin(drag.latestOrigin);
      submitTopology(drag.latestOrigin, side);
    } else {
      // Overlap or no-seam position rejected: snap back cleanly to last valid origin
      setDraftOrigin(drag.lastValidOrigin);
    }
  };

  const activeSide = primarySeamSide(layout.seams, fallbackSide);
  const isCursorRemote = status.is_input_trapped;
  const isCursorLocal = !status.is_input_trapped;
  const deadzonePx = settings.cornerDeadzonePx;

  const deadzoneCanvasPx =
    deadzonePx <= 0
      ? 0
      : Math.min(24, Math.max(4, Math.round(deadzonePx * layout.scale)));

  const snapTransitionClass = isDragging ? "" : "canvas-snap-transition";

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
          <span className="text-[11px] text-text-muted">Preset edge:</span>
          <div className="flex rounded-lg border border-ink-700 bg-ink-950 p-0.5">
            {EDGE_OPTIONS.map(({ side, label }) => (
              <button
                key={side}
                type="button"
                onClick={() => handlePresetSide(side)}
                className={`btn-tactile rounded px-2.5 py-1 text-[10px] font-mono cursor-pointer ${
                  activeSide === side
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

      {monitorChangeNotice && (
        <div
          role="status"
          className="flex items-center justify-between gap-3 rounded-lg border border-status-warning/40 bg-status-warning-bg px-3.5 py-2 text-xs text-status-warning"
        >
          <div className="flex items-center gap-2">
            <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
            <span>{monitorChangeNotice}</span>
          </div>
          <button
            type="button"
            onClick={dismissMonitorChangeNotice}
            aria-label="Dismiss monitor change notice"
            className="btn-tactile rounded p-0.5 text-status-warning hover:text-text-strong cursor-pointer"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      )}

      {/* Scaled Blueprint Grid Canvas */}
      <div
        ref={containerRef}
        className={`relative h-80 w-full rounded-xl border overflow-hidden transition-colors duration-150 ${
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

        {/* Main PC Label + Bounding Guide */}
        <div
          style={{
            left: `${layout.localUnionCanvas.left}px`,
            top: `${layout.localUnionCanvas.top}px`,
            width: `${layout.localUnionCanvas.width}px`,
            height: `${layout.localUnionCanvas.height}px`,
          }}
          className={`absolute pointer-events-none ${snapTransitionClass}`}
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

        {/* Individual Main PC Monitors */}
        {layout.localMonitors.map((mon, idx) => (
          <div
            key={mon.id || idx}
            title={mon.deviceName || mon.id}
            style={{
              left: `${mon.canvas.left}px`,
              top: `${mon.canvas.top}px`,
              width: `${mon.canvas.width}px`,
              height: `${mon.canvas.height}px`,
            }}
            className={`absolute flex flex-col items-center justify-center rounded border bg-ink-800 overflow-hidden pointer-events-none ${snapTransitionClass} ${
              isCursorLocal ? "border-mist-300/70" : "border-mist-300/40"
            }`}
          >
            <span className="font-mono text-[9px] font-semibold text-text-strong truncate max-w-full px-1">
              {shortMonitorLabel(mon.deviceName, mon.isPrimary, idx)}
            </span>
            <span className="font-mono text-[8px] text-mist-300 truncate max-w-full px-1">
              {mon.physical.width}×{mon.physical.height}
            </span>
          </div>
        ))}

        {/* Linked PC Block (Draggable via Pointer Capture) */}
        {peer && (
          <div
            data-testid="linked-pc-block"
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={finishDrag}
            onPointerCancel={finishDrag}
            style={{
              left: `${layout.peerUnionCanvas.left}px`,
              top: `${layout.peerUnionCanvas.top}px`,
              width: `${layout.peerUnionCanvas.width}px`,
              height: `${layout.peerUnionCanvas.height}px`,
              touchAction: "none",
            }}
            className={`absolute pointer-events-none ${snapTransitionClass} ${
              isDragging ? "z-20 opacity-95" : "z-10"
            }`}
          >
            <div className="absolute -bottom-5 left-0 flex items-center gap-1.5 whitespace-nowrap pointer-events-none">
              <span
                className={`font-mono text-[10px] font-bold ${
                  isDragging && dragState.overlaps
                    ? "text-status-danger"
                    : "text-status-success"
                }`}
              >
                {peer.name}
              </span>
              {isCursorRemote && (
                <span className="inline-flex items-center gap-0.5 rounded bg-gold-400 px-1.5 py-0.2 text-[8px] font-mono font-bold text-ink-950">
                  <MousePointer className="h-2 w-2 fill-current" />
                  CURSOR
                </span>
              )}
            </div>

            {/* Individual Linked PC Monitors (hit-testable drag handles) */}
            {layout.peerMonitors.map((mon, idx) => {
              const relLeft = mon.canvas.left - layout.peerUnionCanvas.left;
              const relTop = mon.canvas.top - layout.peerUnionCanvas.top;
              const borderClass =
                isDragging && dragState.overlaps
                  ? "border-status-danger bg-status-danger-bg"
                  : isDragging && !dragState.hasSeam
                    ? "border-status-warning/70 bg-ink-800"
                    : isCursorRemote
                      ? "border-gold-400 bg-status-success-bg"
                      : "border-status-success/60 bg-status-success-bg hover:border-status-success";
              return (
                <div
                  key={mon.id || idx}
                  title={mon.deviceName || mon.id}
                  style={{
                    left: `${relLeft}px`,
                    top: `${relTop}px`,
                    width: `${mon.canvas.width}px`,
                    height: `${mon.canvas.height}px`,
                  }}
                  className={`absolute flex flex-col items-center justify-center rounded border overflow-hidden pointer-events-auto ${snapTransitionClass} ${
                    isDragging ? "cursor-grabbing" : "cursor-grab"
                  } ${borderClass}`}
                >
                  <span className="font-mono text-[9px] font-semibold text-text-strong truncate max-w-full px-1">
                    {mon.isPrimary ? "Primary" : shortMonitorLabel(mon.deviceName, false, idx)}
                  </span>
                  <span
                    className={`font-mono text-[8px] truncate max-w-full px-1 ${
                      isDragging && dragState.overlaps
                        ? "text-status-danger"
                        : "text-status-success"
                    }`}
                  >
                    {mon.physical.width}×{mon.physical.height}
                  </span>
                </div>
              );
            })}

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

        {/* Per-Monitor Gold Seam Highlights + Corner Deadzones */}
        {peer &&
          layout.seamRects.map((item, idx) => {
            const isVertical = item.orientation === "vertical";
            const spanLen = isVertical ? item.canvas.height : item.canvas.width;
            const dzPx =
              deadzoneCanvasPx <= 0
                ? 0
                : Math.min(spanLen / 2, deadzoneCanvasPx);
            return (
              <div
                key={`${item.seam.main_monitor_id}-${item.seam.linked_monitor_id}-${item.seam.side}-${idx}`}
                data-testid="seam-highlight"
                data-seam-side={item.seam.side}
                data-main-monitor={item.seam.main_monitor_id}
                data-linked-monitor={item.seam.linked_monitor_id}
                style={{
                  left: `${item.canvas.left}px`,
                  top: `${item.canvas.top}px`,
                  width: `${item.canvas.width}px`,
                  height: `${item.canvas.height}px`,
                }}
                className={`absolute flex ${
                  isVertical ? "flex-col" : "flex-row"
                } justify-between pointer-events-none z-30 ${snapTransitionClass}`}
              >
                {dzPx > 0 && (
                  <div
                    style={isVertical ? { height: `${dzPx}px` } : { width: `${dzPx}px` }}
                    className={`${
                      isVertical ? "w-full" : "h-full"
                    } rounded-sm bg-status-warning/85 shrink-0`}
                  />
                )}
                <div className="flex-1 bg-gold-400" />
                {dzPx > 0 && (
                  <div
                    style={isVertical ? { height: `${dzPx}px` } : { width: `${dzPx}px` }}
                    className={`${
                      isVertical ? "w-full" : "h-full"
                    } rounded-sm bg-status-warning/85 shrink-0`}
                  />
                )}
              </div>
            );
          })}

        {peer && (
          <div className="absolute bottom-2 right-3 z-30 flex items-center gap-1.5 text-[10px] font-mono pointer-events-none">
            <Move
              className={`h-3 w-3 ${
                isDragging && dragState.overlaps
                  ? "text-status-danger"
                  : "text-text-muted"
              }`}
            />
            {isDragging && dragState.overlaps ? (
              <span className="text-status-danger">
                Overlaps Main PC — release to snap back
              </span>
            ) : isDragging && !dragState.hasSeam ? (
              <span className="text-status-warning">
                Move near a monitor edge to snap
              </span>
            ) : (
              <span className="text-text-muted">
                Drag {peer.name} around monitors • {layout.seams.length} seam
                {layout.seams.length === 1 ? "" : "s"} ({draftOrigin.x}, {draftOrigin.y})
              </span>
            )}
          </div>
        )}
      </div>

      {/* Deadzone & Dwell Controls */}
      {peer && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-ink-700 bg-ink-950 px-4 py-2.5 text-xs">
          <div className="flex flex-wrap items-center gap-4">
            <div className="flex items-center gap-1.5 text-text">
              <span className="h-2 w-2 rounded-full bg-gold-400" />
              <span className="font-mono text-[11px]">Active Seam</span>
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
