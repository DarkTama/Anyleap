export interface ScreenRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface MonitorDisplay {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  is_primary: boolean;
}

export interface ComputerNode {
  id: string;
  name: string;
  is_local: boolean;
  address?: string;
  bounds: ScreenRect;
  monitors?: MonitorDisplay[];
}

export type EdgeSide = "left" | "right" | "top" | "bottom";

export interface TopologyLink {
  from_id: string;
  from_edge: EdgeSide;
  to_id: string;
  to_edge: EdgeSide;
  offset_x?: number;
  offset_y?: number;
  deadzone_px?: number;
  resistance_ms?: number;
}

export interface TopologyConfig {
  computers: ComputerNode[];
  links: TopologyLink[];
}

export interface LinkedPeerInfo {
  computer_id: string;
  name: string;
  role: "main" | "linked";
}

export interface TrustedPeerInfo {
  computer_id: string;
  name: string;
  address: string;
}

export interface KvmStatus {
  is_host_running: boolean;
  allow_control?: boolean;
  port: number;
  security_pin: string;
  connected_peers: string[];
  is_input_trapped: boolean;
  active_target_id?: string;
  linked_peer?: LinkedPeerInfo | null;
  local_name?: string;
  local_id?: string;
  screen_width?: number;
  screen_height?: number;
  monitors?: MonitorDisplay[];
  peers?: ComputerNode[];
  /** Edge of this PC that leaps to the peer while this PC is the active controller. */
  controller_edge?: string | null;
  /** True while a remote controller drives this PC's cursor. */
  receiver_entered?: boolean;
}

export interface DiscoveredPeer {
  id: string;
  name: string;
  ip: string;
  port: number;
  screen_w?: number;
  screen_h?: number;
  last_seen_secs?: number;
}

export interface KvmActivityItem {
  id: string;
  timestamp: number;
  type: "text" | "image" | "file";
  name: string;
  sizeBytes?: number;
  progressPercent: number;
  status: "syncing" | "completed" | "error";
  savedPath?: string;
}

export interface KvmSettingsConfig {
  edgeResistanceMs: number;
  cornerDeadzonePx: number;
  indicatorHud: boolean;
  indicatorHudCorner: "top-left" | "top-right" | "bottom-left" | "bottom-right";
  indicatorCanvasDot: boolean;
  syncClipboardText: boolean;
  syncClipboardImage: boolean;
  syncFileTransfer: boolean;
  debugLog?: boolean;
}
