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

export interface TopologyLink {
  from_id: string;
  from_edge: "left" | "right" | "top" | "bottom";
  to_id: string;
  to_edge: "left" | "right" | "top" | "bottom";
  offset_x?: number;
  offset_y?: number;
  deadzone_px?: number;
  resistance_ms?: number;
}

export interface TopologyConfig {
  computers: ComputerNode[];
  links: TopologyLink[];
}

export interface KvmStatus {
  is_host_running: boolean;
  tcp_port: number;
  udp_port: number;
  security_pin: string;
  connected_peers: string[];
  is_input_trapped: boolean;
  active_target_id?: string;
  local_name?: string;
  local_id?: string;
  screen_width?: number;
  screen_height?: number;
  monitors?: MonitorDisplay[];
}

export interface DiscoveredPeer {
  id: string;
  name: string;
  ip: string;
  tcp_port: number;
  udp_port: number;
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
  indicatorRipple: boolean;
  indicatorHud: boolean;
  indicatorHudCorner: "top-left" | "top-right" | "bottom-left" | "bottom-right";
  indicatorCanvasDot: boolean;
  emergencyKeyCombo: string;
  syncClipboardText: boolean;
  syncClipboardImage: boolean;
  syncFileTransfer: boolean;
}
