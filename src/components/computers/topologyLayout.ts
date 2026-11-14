import type {
  ComputerNode,
  EdgeSide,
  KvmStatus,
  MonitorDisplay,
  Point,
  ScreenRect,
  Seam,
  TopologyLink,
} from "@/types/kvm";

export type ComputerRole = "none" | "main" | "linked";

export interface VisibleSections {
  showStatusCard: boolean;
  showAllowControlToggle: boolean;
  showPairingCode: boolean;
  showDiscoveredPeers: boolean;
  showLayoutCanvas: boolean;
  showEdgeControls: boolean;
  showBringCursorBack: boolean;
  showControlledByCard: boolean;
  showTrustedPeers: boolean;
  showUnlink: boolean;
}

export function getComputerRole(status: KvmStatus): ComputerRole {
  if (status.linked_peer?.role === "main") return "main";
  if (status.linked_peer?.role === "linked") return "linked";
  if (status.controller_edge) return "main";
  if (status.is_host_running && status.connected_peers.length > 0) {
    return "linked";
  }
  return "none";
}

export function getVisibleSections(role: ComputerRole): VisibleSections {
  switch (role) {
    case "main":
      return {
        showStatusCard: true,
        showAllowControlToggle: false,
        showPairingCode: false,
        showDiscoveredPeers: false,
        showLayoutCanvas: true,
        showEdgeControls: true,
        showBringCursorBack: true,
        showControlledByCard: false,
        showTrustedPeers: false,
        showUnlink: true,
      };
    case "linked":
      return {
        showStatusCard: false,
        showAllowControlToggle: false,
        showPairingCode: false,
        showDiscoveredPeers: false,
        showLayoutCanvas: false,
        showEdgeControls: false,
        showBringCursorBack: false,
        showControlledByCard: true,
        showTrustedPeers: true,
        showUnlink: true,
      };
    case "none":
    default:
      return {
        showStatusCard: true,
        showAllowControlToggle: true,
        showPairingCode: true,
        showDiscoveredPeers: true,
        showLayoutCanvas: false,
        showEdgeControls: false,
        showBringCursorBack: false,
        showControlledByCard: false,
        showTrustedPeers: false,
        showUnlink: false,
      };
  }
}

export function oppositeEdge(edge: EdgeSide): EdgeSide {
  switch (edge) {
    case "left":
      return "right";
    case "right":
      return "left";
    case "top":
      return "bottom";
    case "bottom":
      return "top";
  }
}

export function unionRect(monitors: MonitorDisplay[]): ScreenRect {
  if (!monitors || monitors.length === 0) {
    return { x: 0, y: 0, width: 0, height: 0 };
  }
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const m of monitors) {
    minX = Math.min(minX, m.x);
    minY = Math.min(minY, m.y);
    maxX = Math.max(maxX, m.x + m.width);
    maxY = Math.max(maxY, m.y + m.height);
  }
  return {
    x: minX,
    y: minY,
    width: Math.max(0, maxX - minX),
    height: Math.max(0, maxY - minY),
  };
}

export interface CanvasRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface PlacedMonitor {
  id: string;
  deviceName?: string;
  isPrimary: boolean;
  physical: ScreenRect;
  canvas: CanvasRect;
}

export interface SharedEdgeSpan {
  physicalStart: number;
  physicalEnd: number;
  canvas: CanvasRect;
}

export interface SeamCanvasRect {
  seam: Seam;
  orientation: "vertical" | "horizontal";
  canvas: CanvasRect;
}

export interface CanvasViewport {
  scale: number;
  minX: number;
  minY: number;
  originX: number;
  originY: number;
}

export interface CanvasLayoutResult {
  scale: number;
  viewport: CanvasViewport;
  remoteOrigin: Point;
  localUnionPhysical: ScreenRect;
  peerUnionPhysical: ScreenRect;
  localUnionCanvas: CanvasRect;
  peerUnionCanvas: CanvasRect;
  localMonitors: PlacedMonitor[];
  peerMonitors: PlacedMonitor[];
  seams: Seam[];
  seamRects: SeamCanvasRect[];
  sharedEdgeSpans: SharedEdgeSpan[];
}

function isEdgeMonitor(rect: ScreenRect, union: ScreenRect, side: EdgeSide): boolean {
  if (rect.width <= 0 || rect.height <= 0) return false;
  switch (side) {
    case "left":
      return rect.x === union.x;
    case "right":
      return rect.x + rect.width === union.x + union.width;
    case "top":
      return rect.y === union.y;
    case "bottom":
      return rect.y + rect.height === union.y + union.height;
  }
}

function mergeIntervals(intervals: Array<[number, number]>): Array<[number, number]> {
  if (intervals.length === 0) return [];
  const sorted = [...intervals].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const merged: Array<[number, number]> = [sorted[0]];
  for (let i = 1; i < sorted.length; i++) {
    const [start, end] = sorted[i];
    const last = merged[merged.length - 1];
    if (start <= last[1]) {
      last[1] = Math.max(last[1], end);
    } else {
      merged.push([start, end]);
    }
  }
  return merged;
}

export function normalizeMonitors(
  monitors: MonitorDisplay[] | undefined,
  fallbackBounds: ScreenRect | undefined,
  prefix: string
): MonitorDisplay[] {
  if (monitors && monitors.length > 0) {
    return monitors.map((m, idx) => {
      const id = m.id || m.device_name || `${prefix}-${idx + 1}`;
      return {
        ...m,
        id,
        device_name: m.device_name ?? m.id,
      };
    });
  }
  const w = fallbackBounds && fallbackBounds.width > 0 ? fallbackBounds.width : 1920;
  const h = fallbackBounds && fallbackBounds.height > 0 ? fallbackBounds.height : 1080;
  const fallbackId = `${prefix}-fallback`;
  return [
    {
      id: fallbackId,
      device_name: fallbackId,
      x: 0,
      y: 0,
      width: w,
      height: h,
      is_primary: true,
    },
  ];
}

export function monitorLayoutSignature(monitors: MonitorDisplay[] | undefined): string {
  if (!monitors || monitors.length === 0) return "";
  return monitors
    .map((m) => `${m.device_name || m.id}:${m.x},${m.y},${m.width}x${m.height}`)
    .sort()
    .join("|");
}

function rectsStrictlyOverlap(a: ScreenRect, b: ScreenRect): boolean {
  if (a.width <= 0 || a.height <= 0 || b.width <= 0 || b.height <= 0) {
    return false;
  }
  return (
    a.x < b.x + b.width &&
    a.x + a.width > b.x &&
    a.y < b.y + b.height &&
    a.y + a.height > b.y
  );
}

/**
 * Pure TS twin of the Rust seam computation (used by the canvas for drawing).
 * Only touching edges (shared boundary line with positive overlap) form seams.
 */
export function computeSeams(
  mainMonitors: MonitorDisplay[],
  linkedMonitors: MonitorDisplay[],
  remoteOrigin: Point
): Seam[] {
  const seams: Seam[] = [];
  for (const mm of mainMonitors) {
    if (mm.width <= 0 || mm.height <= 0) continue;
    const mainId = mm.id || mm.device_name || "";
    for (const lm of linkedMonitors) {
      if (lm.width <= 0 || lm.height <= 0) continue;
      const linkedId = lm.id || lm.device_name || "";
      const lx = remoteOrigin.x + lm.x;
      const ly = remoteOrigin.y + lm.y;

      if (mm.x === lx + lm.width) {
        const start = Math.max(mm.y, ly);
        const end = Math.min(mm.y + mm.height, ly + lm.height);
        if (end > start) {
          seams.push({
            main_monitor_id: mainId,
            linked_monitor_id: linkedId,
            side: "left",
            start,
            end,
          });
        }
      }

      if (mm.x + mm.width === lx) {
        const start = Math.max(mm.y, ly);
        const end = Math.min(mm.y + mm.height, ly + lm.height);
        if (end > start) {
          seams.push({
            main_monitor_id: mainId,
            linked_monitor_id: linkedId,
            side: "right",
            start,
            end,
          });
        }
      }

      if (mm.y === ly + lm.height) {
        const start = Math.max(mm.x, lx);
        const end = Math.min(mm.x + mm.width, lx + lm.width);
        if (end > start) {
          seams.push({
            main_monitor_id: mainId,
            linked_monitor_id: linkedId,
            side: "top",
            start,
            end,
          });
        }
      }

      if (mm.y + mm.height === ly) {
        const start = Math.max(mm.x, lx);
        const end = Math.min(mm.x + mm.width, lx + lm.width);
        if (end > start) {
          seams.push({
            main_monitor_id: mainId,
            linked_monitor_id: linkedId,
            side: "bottom",
            start,
            end,
          });
        }
      }
    }
  }
  return seams;
}

/**
 * Returns true if placing Linked PC monitors at `remoteOrigin` causes any
 * Linked monitor to strictly overlap any Main monitor. Touching edges do not overlap.
 */
export function hasMonitorOverlap(
  mainMonitors: MonitorDisplay[],
  linkedMonitors: MonitorDisplay[],
  remoteOrigin: Point
): boolean {
  for (const mm of mainMonitors) {
    for (const lm of linkedMonitors) {
      const translated: ScreenRect = {
        x: remoteOrigin.x + lm.x,
        y: remoteOrigin.y + lm.y,
        width: lm.width,
        height: lm.height,
      };
      if (rectsStrictlyOverlap(mm, translated)) {
        return true;
      }
    }
  }
  return false;
}

export interface SnapResult {
  origin: Point;
  snapped: boolean;
  valid: boolean;
  overlaps: boolean;
  seams: Seam[];
  reason?: "overlap" | "no-seam";
}

/**
 * Snaps a candidate `remote_origin` within `maxSnapScreenPx` (default 24px)
 * screen distance so that at least one Linked monitor edge touches a Main
 * monitor edge with positive overlap and zero monitor overlaps.
 */
export function snapRemoteOrigin(params: {
  mainMonitors: MonitorDisplay[];
  linkedMonitors: MonitorDisplay[];
  candidateOrigin: Point;
  scale: number;
  maxSnapScreenPx?: number;
  alignSnapScreenPx?: number;
}): SnapResult {
  const {
    mainMonitors,
    linkedMonitors,
    candidateOrigin,
    scale,
    maxSnapScreenPx = 24,
    alignSnapScreenPx = 8,
  } = params;

  const safeScale = Math.max(scale, 1e-6);
  const touchThresholdPhys = maxSnapScreenPx / safeScale;
  const alignThresholdPhys = alignSnapScreenPx / safeScale;

  const rawX = Math.round(candidateOrigin.x);
  const rawY = Math.round(candidateOrigin.y);

  const xMap = new Map<number, boolean>();
  const yMap = new Map<number, boolean>();
  xMap.set(rawX, false);
  yMap.set(rawY, false);

  const addCand = (map: Map<number, boolean>, val: number, isAlign: boolean) => {
    const rounded = Math.round(val);
    const prev = map.get(rounded);
    if (prev === undefined || (!isAlign && prev)) {
      map.set(rounded, isAlign);
    }
  };

  for (const mm of mainMonitors) {
    for (const lm of linkedMonitors) {
      const touchX1 = mm.x - (lm.x + lm.width);
      const touchX2 = mm.x + mm.width - lm.x;
      if (Math.abs(touchX1 - rawX) <= touchThresholdPhys) {
        addCand(xMap, touchX1, false);
      }
      if (Math.abs(touchX2 - rawX) <= touchThresholdPhys) {
        addCand(xMap, touchX2, false);
      }

      const alignX1 = mm.x - lm.x;
      const alignX2 = mm.x + mm.width - (lm.x + lm.width);
      if (Math.abs(alignX1 - rawX) <= alignThresholdPhys) {
        addCand(xMap, alignX1, true);
      }
      if (Math.abs(alignX2 - rawX) <= alignThresholdPhys) {
        addCand(xMap, alignX2, true);
      }

      const touchY1 = mm.y - (lm.y + lm.height);
      const touchY2 = mm.y + mm.height - lm.y;
      if (Math.abs(touchY1 - rawY) <= touchThresholdPhys) {
        addCand(yMap, touchY1, false);
      }
      if (Math.abs(touchY2 - rawY) <= touchThresholdPhys) {
        addCand(yMap, touchY2, false);
      }

      const alignY1 = mm.y - lm.y;
      const alignY2 = mm.y + mm.height - (lm.y + lm.height);
      if (Math.abs(alignY1 - rawY) <= alignThresholdPhys) {
        addCand(yMap, alignY1, true);
      }
      if (Math.abs(alignY2 - rawY) <= alignThresholdPhys) {
        addCand(yMap, alignY2, true);
      }
    }
  }

  let best: {
    origin: Point;
    seams: Seam[];
    score: number;
  } | null = null;

  for (const [cx, isAlignX] of xMap.entries()) {
    for (const [cy, isAlignY] of yMap.entries()) {
      const distScreen = Math.hypot((cx - rawX) * safeScale, (cy - rawY) * safeScale);
      if (distScreen > maxSnapScreenPx + 1e-6) continue;

      const candidate = { x: cx, y: cy };
      if (hasMonitorOverlap(mainMonitors, linkedMonitors, candidate)) continue;
      const seams = computeSeams(mainMonitors, linkedMonitors, candidate);
      if (seams.length === 0) continue;

      const alignBonus = (isAlignX ? alignSnapScreenPx : 0) + (isAlignY ? alignSnapScreenPx : 0);
      const score = distScreen - alignBonus;
      if (!best || score < best.score) {
        best = { origin: candidate, seams, score };
      }
    }
  }

  if (best) {
    return {
      origin: best.origin,
      snapped: best.origin.x !== rawX || best.origin.y !== rawY,
      valid: true,
      overlaps: false,
      seams: best.seams,
    };
  }

  const rawPoint = { x: rawX, y: rawY };
  const overlaps = hasMonitorOverlap(mainMonitors, linkedMonitors, rawPoint);
  const seams = overlaps ? [] : computeSeams(mainMonitors, linkedMonitors, rawPoint);
  if (!overlaps && seams.length > 0) {
    return {
      origin: rawPoint,
      snapped: false,
      valid: true,
      overlaps: false,
      seams,
    };
  }

  return {
    origin: rawPoint,
    snapped: false,
    valid: false,
    overlaps,
    seams: [],
    reason: overlaps ? "overlap" : "no-seam",
  };
}

export function computePeerOrigin(
  localUnion: ScreenRect,
  peerUnion: ScreenRect,
  edge: EdgeSide,
  offsetPx: number
): Point {
  switch (edge) {
    case "right":
      return { x: localUnion.x + localUnion.width, y: localUnion.y + offsetPx };
    case "left":
      return { x: localUnion.x - peerUnion.width, y: localUnion.y + offsetPx };
    case "bottom":
      return { x: localUnion.x + offsetPx, y: localUnion.y + localUnion.height };
    case "top":
      return { x: localUnion.x + offsetPx, y: localUnion.y - peerUnion.height };
  }
}

/**
 * Computes a valid, non-overlapping, seam-forming `remote_origin` preset on
 * the requested side of Main PC (Left / Right / Top / Bottom).
 */
export function computePresetOrigin(
  mainMonitors: MonitorDisplay[],
  linkedMonitors: MonitorDisplay[],
  side: EdgeSide
): Point {
  const mainMons = normalizeMonitors(mainMonitors, undefined, "local");
  const linkedMons = normalizeMonitors(linkedMonitors, undefined, "peer");
  const mainUnion = unionRect(mainMons);
  const linkedUnion = unionRect(linkedMons);

  const fromUnion = (ux: number, uy: number): Point => ({
    x: Math.round(ux - linkedUnion.x),
    y: Math.round(uy - linkedUnion.y),
  });

  const isValid = (origin: Point) =>
    !hasMonitorOverlap(mainMons, linkedMons, origin) &&
    computeSeams(mainMons, linkedMons, origin).length > 0;

  const unionPos = computePeerOrigin(mainUnion, linkedUnion, side, 0);
  const defaultOrigin = fromUnion(unionPos.x, unionPos.y);
  if (isValid(defaultOrigin)) {
    return defaultOrigin;
  }

  const sortedMains = [...mainMons].sort((a, b) => {
    switch (side) {
      case "left":
        return a.x - b.x;
      case "right":
        return b.x + b.width - (a.x + a.width);
      case "top":
        return a.y - b.y;
      case "bottom":
        return b.y + b.height - (a.y + a.height);
    }
  });

  for (const mm of sortedMains) {
    if (side === "left" || side === "right") {
      const ux = side === "left" ? mm.x - linkedUnion.width : mm.x + mm.width;
      const candidatesY = [
        mm.y,
        mm.y + Math.round((mm.height - linkedUnion.height) / 2),
        mm.y + mm.height - linkedUnion.height,
      ];
      for (const uy of candidatesY) {
        const candidate = fromUnion(ux, uy);
        if (isValid(candidate)) return candidate;
      }
    } else {
      const uy = side === "top" ? mm.y - linkedUnion.height : mm.y + mm.height;
      const candidatesX = [
        mm.x,
        mm.x + Math.round((mm.width - linkedUnion.width) / 2),
        mm.x + mm.width - linkedUnion.width,
      ];
      for (const ux of candidatesX) {
        const candidate = fromUnion(ux, uy);
        if (isValid(candidate)) return candidate;
      }
    }
  }

  return defaultOrigin;
}

/**
 * Resolves `remote_origin` from explicit SPEC-24 `remote_origin`, migrating
 * from a SPEC-23 `TopologyLink` if absent, or falling back to a side preset.
 */
export function resolveRemoteOrigin(params: {
  mainMonitors: MonitorDisplay[];
  linkedMonitors: MonitorDisplay[];
  remoteOrigin?: Point | null;
  link?: Pick<TopologyLink, "from_edge" | "offset_x" | "offset_y"> | null;
  fallbackSide?: EdgeSide;
}): Point {
  const {
    mainMonitors,
    linkedMonitors,
    remoteOrigin,
    link,
    fallbackSide = "right",
  } = params;

  if (remoteOrigin && Number.isFinite(remoteOrigin.x) && Number.isFinite(remoteOrigin.y)) {
    return {
      x: Math.round(remoteOrigin.x),
      y: Math.round(remoteOrigin.y),
    };
  }

  const mainMons = normalizeMonitors(mainMonitors, undefined, "local");
  const linkedMons = normalizeMonitors(linkedMonitors, undefined, "peer");

  if (link) {
    const edge = link.from_edge;
    const isTopBottom = edge === "top" || edge === "bottom";
    const offsetPx = isTopBottom ? (link.offset_x ?? 0) : (link.offset_y ?? 0);
    const mainUnion = unionRect(mainMons);
    const linkedUnion = unionRect(linkedMons);
    const unionOrigin = computePeerOrigin(mainUnion, linkedUnion, edge, offsetPx);
    return {
      x: Math.round(unionOrigin.x - linkedUnion.x),
      y: Math.round(unionOrigin.y - linkedUnion.y),
    };
  }

  return computePresetOrigin(mainMons, linkedMons, fallbackSide);
}

export function primarySeamSide(seams: Seam[], fallbackSide: EdgeSide = "right"): EdgeSide {
  if (seams.length === 0) return fallbackSide;
  let best = seams[0];
  let bestLen = best.end - best.start;
  for (let i = 1; i < seams.length; i++) {
    const len = seams[i].end - seams[i].start;
    if (len > bestLen) {
      best = seams[i];
      bestLen = len;
    }
  }
  return best.side;
}

export function clampEdgeOffset(
  localUnion: ScreenRect,
  peerUnion: ScreenRect,
  edge: EdgeSide,
  offsetPx: number
): number {
  const isHorizontalEdge = edge === "left" || edge === "right";
  const localSpan = isHorizontalEdge ? localUnion.height : localUnion.width;
  const peerSpan = isHorizontalEdge ? peerUnion.height : peerUnion.width;
  const minOverlap = 64;
  const minOffset = peerSpan > minOverlap ? -(peerSpan - minOverlap) : -1000;
  const maxOffset = localSpan > minOverlap ? localSpan - minOverlap : 1000;
  return Math.round(Math.max(minOffset, Math.min(maxOffset, offsetPx)));
}

/**
 * Pure drawing helper: maps `Seam[]` in Main PC physical coordinates into
 * canvas-space rectangles aligned on each Main monitor's boundary edge.
 */
export function computeSeamCanvasRects(params: {
  seams: Seam[];
  localMonitors: PlacedMonitor[];
  viewport: CanvasViewport;
  thickness?: number;
}): SeamCanvasRect[] {
  const { seams, localMonitors, viewport, thickness = 6 } = params;
  const { scale, minX, minY, originX, originY } = viewport;
  const monMap = new Map<string, PlacedMonitor>();
  for (const mon of localMonitors) {
    monMap.set(mon.id, mon);
    if (mon.deviceName) monMap.set(mon.deviceName, mon);
  }

  const results: SeamCanvasRect[] = [];
  for (const seam of seams) {
    const mm = monMap.get(seam.main_monitor_id);
    if (!mm || seam.end <= seam.start) continue;

    if (seam.side === "left" || seam.side === "right") {
      const boundaryX =
        seam.side === "left" ? mm.physical.x : mm.physical.x + mm.physical.width;
      results.push({
        seam,
        orientation: "vertical",
        canvas: {
          left: originX + (boundaryX - minX) * scale - thickness / 2,
          top: originY + (seam.start - minY) * scale,
          width: thickness,
          height: (seam.end - seam.start) * scale,
        },
      });
    } else {
      const boundaryY =
        seam.side === "top" ? mm.physical.y : mm.physical.y + mm.physical.height;
      results.push({
        seam,
        orientation: "horizontal",
        canvas: {
          left: originX + (seam.start - minX) * scale,
          top: originY + (boundaryY - minY) * scale - thickness / 2,
          width: (seam.end - seam.start) * scale,
          height: thickness,
        },
      });
    }
  }
  return results;
}

export function computeCanvasLayout(params: {
  localNode: Pick<ComputerNode, "bounds" | "monitors">;
  peerNode: Pick<ComputerNode, "bounds" | "monitors"> | null;
  remoteOrigin?: Point | null;
  edge?: EdgeSide;
  offsetPx?: number;
  canvasWidth: number;
  canvasHeight: number;
  padding?: number;
  frozenViewport?: CanvasViewport | null;
}): CanvasLayoutResult {
  const {
    localNode,
    peerNode,
    remoteOrigin,
    edge = "right",
    offsetPx = 0,
    canvasWidth,
    canvasHeight,
    padding = 24,
    frozenViewport = null,
  } = params;

  const rawLocalMons = normalizeMonitors(localNode.monitors, localNode.bounds, "local");
  const localUnion = unionRect(rawLocalMons);

  const rawPeerMons = peerNode
    ? normalizeMonitors(peerNode.monitors, peerNode.bounds, "peer")
    : [];
  const rawPeerUnion = peerNode ? unionRect(rawPeerMons) : { x: 0, y: 0, width: 0, height: 0 };

  let effectiveOrigin: Point = { x: 0, y: 0 };
  if (peerNode) {
    if (remoteOrigin) {
      effectiveOrigin = {
        x: Math.round(remoteOrigin.x),
        y: Math.round(remoteOrigin.y),
      };
    } else {
      const unionOrigin = computePeerOrigin(localUnion, rawPeerUnion, edge, offsetPx);
      effectiveOrigin = {
        x: Math.round(unionOrigin.x - rawPeerUnion.x),
        y: Math.round(unionOrigin.y - rawPeerUnion.y),
      };
    }
  }

  const peerUnionPhysical: ScreenRect = peerNode
    ? {
        x: effectiveOrigin.x + rawPeerUnion.x,
        y: effectiveOrigin.y + rawPeerUnion.y,
        width: rawPeerUnion.width,
        height: rawPeerUnion.height,
      }
    : { x: 0, y: 0, width: 0, height: 0 };

  let viewport: CanvasViewport;
  if (frozenViewport) {
    viewport = frozenViewport;
  } else {
    const minX = peerNode ? Math.min(localUnion.x, peerUnionPhysical.x) : localUnion.x;
    const minY = peerNode ? Math.min(localUnion.y, peerUnionPhysical.y) : localUnion.y;
    const maxX = peerNode
      ? Math.max(localUnion.x + localUnion.width, peerUnionPhysical.x + peerUnionPhysical.width)
      : localUnion.x + localUnion.width;
    const maxY = peerNode
      ? Math.max(localUnion.y + localUnion.height, peerUnionPhysical.y + peerUnionPhysical.height)
      : localUnion.y + localUnion.height;

    const totalW = Math.max(1, maxX - minX);
    const totalH = Math.max(1, maxY - minY);
    const availW = Math.max(1, canvasWidth - padding * 2);
    const availH = Math.max(1, canvasHeight - padding * 2);
    const scale = Math.min(availW / totalW, availH / totalH);

    const originX = (canvasWidth - totalW * scale) / 2;
    const originY = (canvasHeight - totalH * scale) / 2;
    viewport = { scale, minX, minY, originX, originY };
  }

  const { scale, minX, minY, originX, originY } = viewport;

  const toCanvas = (r: ScreenRect): CanvasRect => ({
    left: originX + (r.x - minX) * scale,
    top: originY + (r.y - minY) * scale,
    width: r.width * scale,
    height: r.height * scale,
  });

  const localMonitors: PlacedMonitor[] = rawLocalMons.map((m) => {
    const physical: ScreenRect = { x: m.x, y: m.y, width: m.width, height: m.height };
    return {
      id: m.id,
      deviceName: m.device_name,
      isPrimary: m.is_primary,
      physical,
      canvas: toCanvas(physical),
    };
  });

  const peerMonitors: PlacedMonitor[] = peerNode
    ? rawPeerMons.map((m) => {
        const physical: ScreenRect = {
          x: effectiveOrigin.x + m.x,
          y: effectiveOrigin.y + m.y,
          width: m.width,
          height: m.height,
        };
        return {
          id: m.id,
          deviceName: m.device_name,
          isPrimary: m.is_primary,
          physical,
          canvas: toCanvas(physical),
        };
      })
    : [];

  const localUnionCanvas = toCanvas(localUnion);
  const peerUnionCanvas = peerNode
    ? toCanvas(peerUnionPhysical)
    : { left: 0, top: 0, width: 0, height: 0 };

  const seams: Seam[] = peerNode
    ? computeSeams(rawLocalMons, rawPeerMons, effectiveOrigin)
    : [];

  const seamRects = computeSeamCanvasRects({
    seams,
    localMonitors,
    viewport,
    thickness: 6,
  });

  const sharedEdgeSpans: SharedEdgeSpan[] = [];
  if (peerNode) {
    const localEdgeMons = localMonitors.filter((m) =>
      isEdgeMonitor(m.physical, localUnion, edge)
    );
    const peerEdgeSide = oppositeEdge(edge);
    const peerEdgeMons = peerMonitors.filter((m) =>
      isEdgeMonitor(m.physical, peerUnionPhysical, peerEdgeSide)
    );
    const isHorizEdge = edge === "left" || edge === "right";
    const rawSpans: Array<[number, number]> = [];
    for (const lm of localEdgeMons) {
      for (const pm of peerEdgeMons) {
        const start = isHorizEdge
          ? Math.max(lm.physical.y, pm.physical.y)
          : Math.max(lm.physical.x, pm.physical.x);
        const end = isHorizEdge
          ? Math.min(lm.physical.y + lm.physical.height, pm.physical.y + pm.physical.height)
          : Math.min(lm.physical.x + lm.physical.width, pm.physical.x + pm.physical.width);
        if (end > start) {
          rawSpans.push([start, end]);
        }
      }
    }

    const seamThickness = 6;
    for (const [physicalStart, physicalEnd] of mergeIntervals(rawSpans)) {
      const canvas: CanvasRect = isHorizEdge
        ? {
            left:
              edge === "left"
                ? localUnionCanvas.left - seamThickness / 2
                : localUnionCanvas.left + localUnionCanvas.width - seamThickness / 2,
            top: originY + (physicalStart - minY) * scale,
            width: seamThickness,
            height: (physicalEnd - physicalStart) * scale,
          }
        : {
            left: originX + (physicalStart - minX) * scale,
            top:
              edge === "top"
                ? localUnionCanvas.top - seamThickness / 2
                : localUnionCanvas.top + localUnionCanvas.height - seamThickness / 2,
            width: (physicalEnd - physicalStart) * scale,
            height: seamThickness,
          };
      sharedEdgeSpans.push({ physicalStart, physicalEnd, canvas });
    }
  }

  return {
    scale,
    viewport,
    remoteOrigin: effectiveOrigin,
    localUnionPhysical: localUnion,
    peerUnionPhysical,
    localUnionCanvas,
    peerUnionCanvas,
    localMonitors,
    peerMonitors,
    seams,
    seamRects,
    sharedEdgeSpans,
  };
}
