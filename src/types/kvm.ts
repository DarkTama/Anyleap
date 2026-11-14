export interface ScreenRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Point {
  x: number;
  y: number;
}

export interface MonitorDisplay {
  id: string;
  device_name?: string;
  x: number;
  y: number;
  width: number;
  height: number;
  is_primary: boolean;
}

export interface Seam {
  main_monitor_id: string;
  linked_monitor_id: string;
  side: EdgeSide;
  start: number;
  end: number;
}

export interface LinkedSeam {
  linked_monitor_id: string;
  side: EdgeSide;
  start: number;
  end: number;
  main_monitor_id: string;
  main_offset: number;
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
  remote_origin?: Point | null;
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
  /** Active per-monitor seams if surfaced by backend status. */
  seams?: Seam[];
  /** Saved Linked PC origin in Main PC virtual-desktop coords if surfaced. */
  remote_origin?: Point | null;
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
  // SPEC-25a: Files & Pointer
  continuityDragDropExperimental: boolean;
  continuityFileClipboard: boolean;
  continuityStagingLimitMb: number;
  continuityClipPrefetchLimitMb: number;
  continuityPointerScheme: boolean;
  // SPEC-25b: Audio & Media Keys
  continuityAudioBridge: boolean;
  continuityAudioVolume: number;
  continuityMediaKeysFollowAudio: boolean;
  continuityMuteLinkedWhileBridging: boolean;
  // SPEC-25c: Session & Open on Linked PC
  continuityLockTogether: boolean;
  continuitySleepTogether: boolean;
  continuityOpenOnLinkedMenu: boolean;
}

export type TransferKind = "drag" | "clip" | "open";

export type TransferProgressState =
  | "awaiting_fetch"
  | "active"
  | "done"
  | "failed"
  | "cancelled";

export interface KvmTransferProgressPayload {
  id: string;
  kind: TransferKind;
  name: string;
  pct: number;
  state?: TransferProgressState;
  sizeBytes?: number;
}

export type LinkedMuteState =
  | "unsupported"
  | "muted"
  | "unmuted"
  | "unknown";

export interface KvmAudioStatePayload {
  streaming: boolean;
  level: number;
  linkedMute?: LinkedMuteState;
}

export interface KvmContinuityNoticePayload {
  text: string;
}

export interface ContinuityNotice {
  id: string;
  text: string;
  timestamp: number;
}
