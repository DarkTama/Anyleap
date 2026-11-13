import { create } from "zustand";
import type {
  ComputerNode,
  DiscoveredPeer,
  KvmActivityItem,
  KvmSettingsConfig,
  KvmStatus,
  MonitorDisplay,
  ScreenRect,
  TopologyConfig,
} from "@/types/kvm";
import {
  kvmGetStatus,
  kvmStartServer,
  kvmStopServer,
  kvmGetDiscoveredPeers,
  kvmConnectPeer,
  kvmDisconnectPeer,
  kvmUpdateTopology,
  kvmEmergencyRelease,
  kvmSendFile,
} from "@/services/kvmService";
import { listen } from "@tauri-apps/api/event";
import { logActivity } from "./activityStore";

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

export interface KvmState {
  status: KvmStatus;
  discoveredPeers: DiscoveredPeer[];
  topology: TopologyConfig;
  selectedPeer: DiscoveredPeer | null;
  isPairingModalOpen: boolean;
  isLoading: boolean;
  error: string | null;

  settings: KvmSettingsConfig;
  activityFeed: KvmActivityItem[];
  cursorPos: { normX: number; normY: number } | null;

  init: () => Promise<void>;
  refreshStatus: () => Promise<void>;
  startServer: () => Promise<void>;
  stopServer: () => Promise<void>;
  refreshDiscovered: () => Promise<void>;
  connectPeer: (peer: DiscoveredPeer) => Promise<void>;
  disconnectPeer: () => Promise<void>;
  setTopology: (topology: TopologyConfig) => Promise<void>;
  emergencyRelease: () => Promise<void>;
  sendFile: (filePath: string) => Promise<void>;
  openPairingModal: (peer?: DiscoveredPeer) => void;
  closePairingModal: () => void;
  setError: (err: string | null) => void;
  updateSettings: (partial: Partial<KvmSettingsConfig>) => void;
  addActivity: (item: KvmActivityItem) => void;
  updateActivityProgress: (
    id: string,
    progress: number,
    status?: "syncing" | "completed" | "error"
  ) => void;
  setCursorPos: (pos: { normX: number; normY: number } | null) => void;
  reset: () => void;
}

const defaultStatus: KvmStatus = {
  is_host_running: false,
  port: 4177,
  security_pin: "",
  connected_peers: [],
  is_input_trapped: false,
  active_target_id: undefined,
  local_name: "This PC",
  local_id: "local-host",
  screen_width: 0,
  screen_height: 0,
  monitors: [],
  peers: [],
};

const defaultSettings: KvmSettingsConfig = {
  edgeResistanceMs: 120,
  cornerDeadzonePx: 48,
  indicatorRipple: true,
  indicatorHud: true,
  indicatorHudCorner: "top-right",
  indicatorCanvasDot: true,
  emergencyKeyCombo: "Ctrl + Alt + Escape",
  syncClipboardText: true,
  syncClipboardImage: true,
  syncFileTransfer: true,
};

const defaultTopology: TopologyConfig = {
  computers: [
    {
      id: "local-host",
      name: "This PC",
      is_local: true,
      bounds: { x: 0, y: 0, width: 0, height: 0 },
      monitors: [],
    },
  ],
  links: [],
};

function loadSavedSettings(): KvmSettingsConfig {
  if (typeof window === "undefined") return defaultSettings;
  try {
    const raw = localStorage.getItem("anyleap_kvm_settings");
    if (raw) {
      return { ...defaultSettings, ...JSON.parse(raw) };
    }
  } catch {
    // fallback
  }
  return defaultSettings;
}

function saveSettings(settings: KvmSettingsConfig) {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem("anyleap_kvm_settings", JSON.stringify(settings));
  } catch {
    // ignore
  }
}

function loadSavedPlacement(): "left" | "right" {
  if (typeof window === "undefined") return "right";
  try {
    const raw = localStorage.getItem("anyleap_kvm_placement");
    if (raw === "left" || raw === "right") return raw;
  } catch {}
  return "right";
}

function savePlacement(placement: "left" | "right") {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem("anyleap_kvm_placement", placement);
  } catch {}
}

function mergePeers(existing: ComputerNode[], statusPeers: ComputerNode[] | undefined): ComputerNode[] {
  const local = existing.find((c) => c.is_local) || existing[0];
  const remoteMap = new Map<string, ComputerNode>();
  for (const c of existing) {
    if (!c.is_local) remoteMap.set(c.id, c);
  }
  if (statusPeers) {
    for (const sp of statusPeers) {
      if (sp.is_local) continue;
      const prev = remoteMap.get(sp.id);
      const bounds = sp.monitors && sp.monitors.length > 0 ? unionRect(sp.monitors) : sp.bounds;
      remoteMap.set(sp.id, prev ? { ...prev, ...sp, bounds } : { ...sp, bounds });
    }
  }
  return [local, ...remoteMap.values()];
}

let eventListening = false;

export const useKvmStore = create<KvmState>((set, get) => ({
  status: defaultStatus,
  discoveredPeers: [],
  topology: defaultTopology,
  selectedPeer: null,
  isPairingModalOpen: false,
  isLoading: false,
  error: null,
  settings: loadSavedSettings(),
  activityFeed: [],
  cursorPos: null,

  init: async () => {
    try {
      const status = await kvmGetStatus();
      const currentTopology = get().topology;
      const primary = currentTopology.computers.find((c) => c.is_local) || currentTopology.computers[0];
      const localMonitors = status.monitors || [];
      const localUnion =
        localMonitors.length > 0
          ? unionRect(localMonitors)
          : {
              x: 0,
              y: 0,
              width: status.screen_width ?? 0,
              height: status.screen_height ?? 0,
            };

      const updatedPrimary: ComputerNode = {
        ...primary,
        id: status.local_id || primary.id,
        name: status.local_name ? `${status.local_name} (This PC)` : "This PC",
        bounds: localUnion,
        monitors: localMonitors,
      };

      const computers = mergePeers(
        [updatedPrimary, ...currentTopology.computers.filter((c) => !c.is_local)],
        status.peers
      );

      set({
        status,
        topology: {
          ...currentTopology,
          computers,
        },
      });

      if (!eventListening && typeof window !== "undefined") {
        eventListening = true;
        void listen<string>("kvm-peer-connected", () => {
          void get().refreshStatus();
        });
        void listen<string | null>("kvm-peer-disconnected", (event) => {
          const disconnectedId = typeof event.payload === "string" ? event.payload : null;
          const cur = get().topology;
          const primaryNode = cur.computers.find((c) => c.is_local) || cur.computers[0];
          if (disconnectedId) {
            set((state) => ({
              selectedPeer: state.selectedPeer?.id === disconnectedId ? null : state.selectedPeer,
              topology: {
                computers: cur.computers.filter((c) => c.is_local || c.id !== disconnectedId),
                links: cur.links.filter(
                  (l) => l.from_id !== disconnectedId && l.to_id !== disconnectedId
                ),
              },
            }));
          } else {
            set({
              selectedPeer: null,
              topology: {
                computers: [primaryNode],
                links: [],
              },
            });
          }
          void get().refreshStatus();
        });
        void listen<string>("kvm-cursor-enter", () => {
          set((state) => ({ status: { ...state.status, is_input_trapped: true } }));
        });
        void listen<string>("kvm-cursor-return", () => {
          set((state) => ({
            status: { ...state.status, is_input_trapped: false },
            cursorPos: null,
          }));
        });
        void listen<{ norm_x: number; norm_y: number }>("kvm-cursor-pos", (event) => {
          set({ cursorPos: { normX: event.payload.norm_x, normY: event.payload.norm_y } });
        });
        void listen<KvmActivityItem>("kvm-activity", (event) => {
          get().addActivity(event.payload);
        });
      }
    } catch {
      // best-effort
    }
  },

  refreshStatus: async () => {
    try {
      const status = await kvmGetStatus();
      const currentTopology = get().topology;
      const updatedComputers = mergePeers(currentTopology.computers, status.peers);

      set({
        status,
        error: null,
        topology: {
          ...currentTopology,
          computers: updatedComputers,
        },
      });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },

  startServer: async () => {
    set({ isLoading: true, error: null });
    try {
      const status = await kvmStartServer();
      set({ status, isLoading: false });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), isLoading: false });
    }
  },

  stopServer: async () => {
    set({ isLoading: true, error: null });
    try {
      await kvmStopServer();
      const status = await kvmGetStatus();
      set({ status, isLoading: false });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), isLoading: false });
    }
  },

  refreshDiscovered: async () => {
    try {
      const peers = await kvmGetDiscoveredPeers();
      set({ discoveredPeers: peers });
    } catch {
      // silent
    }
  },

  connectPeer: async (peer: DiscoveredPeer) => {
    set({ isLoading: true, error: null });
    try {
      const peerIp = peer.ip;
      await kvmConnectPeer(peerIp, peer.port);

      const latestStatus = await kvmGetStatus().catch(() => null);
      const status = latestStatus ?? get().status;
      const currentTopology = get().topology;
      const primary = currentTopology.computers.find((c) => c.is_local) || currentTopology.computers[0];
      const primaryId = status.local_id || primary.id;

      const matchedRemote =
        latestStatus?.peers?.find((p) => !p.is_local && (p.address === peerIp || p.id === peer.id)) ??
        latestStatus?.peers?.find((p) => !p.is_local);

      const peerMonitors = matchedRemote?.monitors ?? [];
      const peerUnion =
        peerMonitors.length > 0
          ? unionRect(peerMonitors)
          : matchedRemote?.bounds ?? {
              x: 0,
              y: 0,
              width: peer.screen_w ?? 0,
              height: peer.screen_h ?? 0,
            };

      const savedPlacement = loadSavedPlacement();
      const peerId = matchedRemote?.id || peer.id;
      const peerName = matchedRemote?.name || peer.name;

      const peerNode: ComputerNode = {
        id: peerId,
        name: peerName,
        is_local: false,
        address: peerIp,
        bounds: {
          x:
            savedPlacement === "right"
              ? primary.bounds.x + primary.bounds.width
              : primary.bounds.x - peerUnion.width,
          y: 0,
          width: peerUnion.width,
          height: peerUnion.height,
        },
        monitors: peerMonitors,
      };

      const updatedComputers = [
        {
          ...primary,
          id: primaryId,
        },
        peerNode,
      ];

      const settings = get().settings;
      const nextTopology: TopologyConfig = {
        computers: updatedComputers,
        links: [
          {
            from_id: primaryId,
            from_edge: savedPlacement,
            to_id: peerId,
            to_edge: savedPlacement === "right" ? "left" : "right",
            deadzone_px: settings.cornerDeadzonePx,
            resistance_ms: settings.edgeResistanceMs,
          },
        ],
      };

      await kvmUpdateTopology(nextTopology);
      set({
        status,
        topology: nextTopology,
        selectedPeer: { ...peer, id: peerId, name: peerName },
        isLoading: false,
      });
      logActivity({
        kind: "kvm",
        title: `Connected KVM peer ${peerName}`,
        detail: peerIp,
      });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), isLoading: false });
    }
  },

  disconnectPeer: async () => {
    set({ isLoading: true });
    try {
      const prevPeer = get().selectedPeer;
      await kvmDisconnectPeer();
      const currentTopology = get().topology;
      const primary = currentTopology.computers.find((c) => c.is_local) || currentTopology.computers[0];

      const resetTopology: TopologyConfig = prevPeer
        ? {
            computers: currentTopology.computers.filter(
              (c) => c.is_local || c.id !== prevPeer.id
            ),
            links: currentTopology.links.filter(
              (l) => l.from_id !== prevPeer.id && l.to_id !== prevPeer.id
            ),
          }
        : {
            computers: [primary],
            links: [],
          };
      await kvmUpdateTopology(resetTopology);
      set({ selectedPeer: null, topology: resetTopology, cursorPos: null, isLoading: false });
      logActivity({
        kind: "kvm",
        title: prevPeer ? `Disconnected KVM peer ${prevPeer.name}` : "Disconnected KVM peer",
      });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), isLoading: false });
    }
  },

  setTopology: async (topology: TopologyConfig) => {
    set({ topology });
    if (topology.links[0]?.from_edge) {
      savePlacement(topology.links[0].from_edge as "left" | "right");
    }
    try {
      await kvmUpdateTopology(topology);
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },

  emergencyRelease: async () => {
    try {
      await kvmEmergencyRelease();
      const status = await kvmGetStatus();
      set({ status, cursorPos: null });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },

  sendFile: async (filePath: string) => {
    try {
      await kvmSendFile(filePath);
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },

  openPairingModal: (peer?: DiscoveredPeer) =>
    set({ isPairingModalOpen: true, selectedPeer: peer ?? null }),

  closePairingModal: () =>
    set({ isPairingModalOpen: false }),

  setError: (err) => set({ error: err }),

  updateSettings: (partial) => {
    const next = { ...get().settings, ...partial };
    set({ settings: next });
    saveSettings(next);

    const currentTopology = get().topology;
    if (currentTopology.links.length > 0) {
      const updatedLinks = currentTopology.links.map((link) => ({
        ...link,
        deadzone_px: next.cornerDeadzonePx,
        resistance_ms: next.edgeResistanceMs,
      }));
      void get().setTopology({ ...currentTopology, links: updatedLinks });
    }
  },

  addActivity: (item) => {
    set((state) => ({
      activityFeed: [item, ...state.activityFeed.slice(0, 49)],
    }));
  },

  updateActivityProgress: (id, progress, status) => {
    set((state) => ({
      activityFeed: state.activityFeed.map((item) =>
        item.id === id
          ? {
              ...item,
              progressPercent: progress,
              ...(status ? { status } : {}),
            }
          : item
      ),
    }));
  },

  setCursorPos: (pos) => set({ cursorPos: pos }),

  reset: () =>
    set({
      status: defaultStatus,
      discoveredPeers: [],
      topology: defaultTopology,
      selectedPeer: null,
      isPairingModalOpen: false,
      isLoading: false,
      error: null,
      activityFeed: [],
      cursorPos: null,
    }),
}));
