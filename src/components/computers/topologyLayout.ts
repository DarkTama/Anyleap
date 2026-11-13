import type {
  ComputerNode,
  EdgeSide,
  KvmStatus,
  MonitorDisplay,
  ScreenRect,
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
  isPrimary: boolean;
  physical: ScreenRect;
  canvas: CanvasRect;
}

export interface SharedEdgeSpan {
  physicalStart: number;
  physicalEnd: number;
  canvas: CanvasRect;
}

export interface CanvasLayoutResult {
  scale: number;
  localUnionPhysical: ScreenRect;
  peerUnionPhysical: ScreenRect;
  localUnionCanvas: CanvasRect;
  peerUnionCanvas: CanvasRect;
  localMonitors: PlacedMonitor[];
  peerMonitors: PlacedMonitor[];
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

function normalizeMonitors(
  monitors: MonitorDisplay[] | undefined,
  fallbackBounds: ScreenRect | undefined,
  prefix: string
): MonitorDisplay[] {
  if (monitors && monitors.length > 0) {
    return monitors;
  }
  const w = fallbackBounds && fallbackBounds.width > 0 ? fallbackBounds.width : 1920;
  const h = fallbackBounds && fallbackBounds.height > 0 ? fallbackBounds.height : 1080;
  return [{ id: `${prefix}-fallback`, x: 0, y: 0, width: w, height: h, is_primary: true }];
}

export function computePeerOrigin(
  localUnion: ScreenRect,
  peerUnion: ScreenRect,
  edge: EdgeSide,
  offsetPx: number
): { x: number; y: number } {
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

export function computeCanvasLayout(params: {
  localNode: Pick<ComputerNode, "bounds" | "monitors">;
  peerNode: Pick<ComputerNode, "bounds" | "monitors"> | null;
  edge: EdgeSide;
  offsetPx: number;
  canvasWidth: number;
  canvasHeight: number;
  padding?: number;
}): CanvasLayoutResult {
  const {
    localNode,
    peerNode,
    edge,
    offsetPx,
    canvasWidth,
    canvasHeight,
    padding = 24,
  } = params;

  const rawLocalMons = normalizeMonitors(localNode.monitors, localNode.bounds, "local");
  const localUnion = unionRect(rawLocalMons);

  const rawPeerMons = peerNode
    ? normalizeMonitors(peerNode.monitors, peerNode.bounds, "peer")
    : [];
  const rawPeerUnion = peerNode ? unionRect(rawPeerMons) : { x: 0, y: 0, width: 0, height: 0 };

  const peerOrigin = peerNode
    ? computePeerOrigin(localUnion, rawPeerUnion, edge, offsetPx)
    : { x: 0, y: 0 };

  const peerUnionPhysical: ScreenRect = peerNode
    ? {
        x: peerOrigin.x,
        y: peerOrigin.y,
        width: rawPeerUnion.width,
        height: rawPeerUnion.height,
      }
    : { x: 0, y: 0, width: 0, height: 0 };

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
      isPrimary: m.is_primary,
      physical,
      canvas: toCanvas(physical),
    };
  });

  const peerMonitors: PlacedMonitor[] = peerNode
    ? rawPeerMons.map((m) => {
        const physical: ScreenRect = {
          x: peerOrigin.x + (m.x - rawPeerUnion.x),
          y: peerOrigin.y + (m.y - rawPeerUnion.y),
          width: m.width,
          height: m.height,
        };
        return {
          id: m.id,
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
    localUnionPhysical: localUnion,
    peerUnionPhysical,
    localUnionCanvas,
    peerUnionCanvas,
    localMonitors,
    peerMonitors,
    sharedEdgeSpans,
  };
}
