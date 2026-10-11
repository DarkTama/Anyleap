import { create } from "zustand";
import type {
  DiscoveredPeer,
  KvmActivityItem,
  KvmSettingsConfig,
  KvmStatus,
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
  tcp_port: 4177,
  udp_port: 4178,
  security_pin: "------",
  connected_peers: [],
  is_input_trapped: false,
  active_target_id: undefined,
  local_name: "This PC",
  local_id: "local-host",
  screen_width: 1920,
  screen_height: 1080,
  monitors: [],
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
      bounds: { x: 0, y: 0, width: 1920, height: 1080 },
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

      const updatedPrimary = {
        ...primary,
        id: status.local_id || "local-host",
        name: status.local_name ? `${status.local_name} (This PC)` : "This PC",
        bounds: {
          x: 0,
          y: 0,
          width: status.screen_width || 1920,
          height: status.screen_height || 1080,
        },
        monitors: status.monitors || [],
      };

      set({
        status,
        topology: {
          ...currentTopology,
          computers: [updatedPrimary, ...currentTopology.computers.filter((c) => !c.is_local)],
        },
      });

      if (!eventListening && typeof window !== "undefined") {
        eventListening = true;
        void listen<string>("kvm-peer-connected", () => {
          void get().refreshStatus();
        });
        void listen("kvm-peer-disconnected", () => {
          const currentTopology = get().topology;
          const primary = currentTopology.computers.find((c) => c.is_local) || currentTopology.computers[0];
          set({
            selectedPeer: null,
            topology: {
              computers: [primary],
              links: [],
            },
          });
          void get().refreshStatus();
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
      const primary = currentTopology.computers.find((c) => c.is_local) || currentTopology.computers[0];

      let updatedComputers = [...currentTopology.computers];
      if (status.connected_peers.length > 0) {
        const peerName = status.connected_peers[0];
        const existing = updatedComputers.find((c) => !c.is_local);
        if (!existing) {
          updatedComputers.push({
            id: `peer-${peerName.toLowerCase().replace(/[^a-z0-9]/g, "-")}`,
            name: peerName,
            is_local: false,
            bounds: {
              x: primary.bounds.x - 1920,
              y: 0,
              width: 1920,
              height: 1080,
            },
          });
        }
      }

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
      await kvmConnectPeer(peerIp, peer.tcp_port, peer.udp_port);

      const status = get().status;
      const currentTopology = get().topology;
      const primary = currentTopology.computers.find((c) => c.is_local) || currentTopology.computers[0];
      const primaryId = status.local_id || primary.id;

      const peerWidth = peer.screen_w || 1920;
      const peerHeight = peer.screen_h || 1080;
      const savedPlacement = loadSavedPlacement();

      const peerNode = {
        id: peer.id,
        name: peer.name,
        is_local: false,
        address: peerIp,
        bounds: {
          x: savedPlacement === "right" ? primary.bounds.x + primary.bounds.width : primary.bounds.x - peerWidth,
          y: 0,
          width: peerWidth,
          height: peerHeight,
        },
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
            to_id: peer.id,
            to_edge: savedPlacement === "right" ? "left" : "right",
            deadzone_px: settings.cornerDeadzonePx,
            resistance_ms: settings.edgeResistanceMs,
          },
        ],
      };

      await kvmUpdateTopology(nextTopology);
      set({ topology: nextTopology, selectedPeer: peer, isLoading: false });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), isLoading: false });
    }
  },

  disconnectPeer: async () => {
    set({ isLoading: true });
    try {
      await kvmDisconnectPeer();
      const currentTopology = get().topology;
      const primary = currentTopology.computers.find((c) => c.is_local) || currentTopology.computers[0];

      const resetTopology: TopologyConfig = {
        computers: [primary],
        links: [],
      };
      await kvmUpdateTopology(resetTopology);
      set({ selectedPeer: null, topology: resetTopology, isLoading: false });
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
      set({ status });
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
