import { create } from "zustand";
import type {
  ComputerNode,
  DiscoveredPeer,
  EdgeSide,
  KvmActivityItem,
  KvmSettingsConfig,
  KvmStatus,
  MonitorDisplay,
  ScreenRect,
  TopologyConfig,
  TopologyLink,
  TrustedPeerInfo,
} from "@/types/kvm";
import {
  kvmGetStatus,
  kvmSetAllowControl,
  kvmStartServer,
  kvmStopServer,
  kvmGetDiscoveredPeers,
  kvmListTrusted,
  kvmConnectPeer,
  kvmDisconnectPeer,
  kvmForgetPeer,
  kvmUpdateTopology,
  kvmEmergencyRelease,
  kvmSendFile,
  kvmSetSyncSettings,
  kvmSetDebugLog,
} from "@/services/kvmService";
import { listen } from "@tauri-apps/api/event";
import { logActivity } from "./activityStore";
import {
  computePeerOrigin,
  oppositeEdge,
  unionRect as layoutUnionRect,
} from "@/components/computers/topologyLayout";

export function unionRect(monitors: MonitorDisplay[]): ScreenRect {
  return layoutUnionRect(monitors);
}

export interface KvmState {
  status: KvmStatus;
  discoveredPeers: DiscoveredPeer[];
  trustedPeers: TrustedPeerInfo[];
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
  setAllowControl: (enabled: boolean) => Promise<void>;
  startServer: () => Promise<void>;
  stopServer: () => Promise<void>;
  refreshDiscovered: () => Promise<void>;
  refreshTrusted: () => Promise<void>;
  connectPeer: (peer: DiscoveredPeer) => Promise<void>;
  disconnectPeer: () => Promise<void>;
  forgetPeer: (computerId: string) => Promise<void>;
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
  allow_control: false,
  port: 4177,
  security_pin: "",
  connected_peers: [],
  is_input_trapped: false,
  active_target_id: undefined,
  linked_peer: null,
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
  indicatorHud: true,
  indicatorHudCorner: "top-right",
  indicatorCanvasDot: true,
  syncClipboardText: true,
  syncClipboardImage: true,
  syncFileTransfer: true,
  debugLog: false,
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

export function loadAllowControl(): boolean {
  if (typeof window === "undefined") return true;
  try {
    const raw = localStorage.getItem("anyleap_kvm_allow_control");
    if (raw === null) return true;
    return raw !== "false";
  } catch {
    return true;
  }
}

export function saveAllowControl(enabled: boolean) {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem("anyleap_kvm_allow_control", String(enabled));
  } catch {
    // ignore
  }
}

function loadSavedPlacement(): EdgeSide {
  if (typeof window === "undefined") return "right";
  try {
    const raw = localStorage.getItem("anyleap_kvm_placement");
    if (raw === "left" || raw === "right" || raw === "top" || raw === "bottom") {
      return raw;
    }
  } catch {}
  return "right";
}

function savePlacement(placement: EdgeSide) {
  if (typeof window === "undefined") return;
  try {
    localStorage.setItem("anyleap_kvm_placement", placement);
  } catch {}
}

function mergePeers(existing: ComputerNode[], statusPeers: ComputerNode[] | undefined): ComputerNode[] {
  const local = existing.find((c) => c.is_local) || existing[0];
  const prevMap = new Map<string, ComputerNode>();
  for (const c of existing) {
    if (!c.is_local) prevMap.set(c.id, c);
  }
  const nextRemotes: ComputerNode[] = [];
  for (const sp of statusPeers ?? []) {
    if (sp.is_local) continue;
    const prev = prevMap.get(sp.id);
    const bounds = sp.monitors && sp.monitors.length > 0 ? unionRect(sp.monitors) : sp.bounds;
    nextRemotes.push(prev ? { ...prev, ...sp, bounds } : { ...sp, bounds });
  }
  return [local, ...nextRemotes];
}

function pruneLinks(links: TopologyLink[], computers: ComputerNode[]): TopologyLink[] {
  const ids = new Set(computers.map((c) => c.id));
  return links.filter((l) => ids.has(l.from_id) && ids.has(l.to_id));
}

let eventListening = false;

export const useKvmStore = create<KvmState>((set, get) => ({
  status: defaultStatus,
  discoveredPeers: [],
  trustedPeers: [],
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
      const [status, trustedPeers] = await Promise.all([
        kvmGetStatus(),
        kvmListTrusted().catch(() => [] as TrustedPeerInfo[]),
      ]);

      set((state) => {
        const currentTopology = state.topology;
        const primary =
          currentTopology.computers.find((c) => c.is_local) ||
          currentTopology.computers[0];
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
        const links = pruneLinks(currentTopology.links, computers);

        return {
          status,
          trustedPeers,
          topology: {
            computers,
            links,
          },
        };
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
        // Both roles emit these events: a controller leaves/returns, a receiver is entered/left.
        void listen<string>("kvm-cursor-enter", () => {
          set((state) =>
            state.status.controller_edge
              ? { status: { ...state.status, is_input_trapped: true } }
              : { status: { ...state.status, receiver_entered: true } }
          );
        });
        void listen<string>("kvm-cursor-return", () => {
          set((state) => ({
            status: { ...state.status, is_input_trapped: false, receiver_entered: false },
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
      set((state) => {
        const currentTopology = state.topology;
        const primary =
          currentTopology.computers.find((c) => c.is_local) ||
          currentTopology.computers[0];
        const localMonitors = status.monitors ?? primary.monitors ?? [];
        const localUnion =
          localMonitors.length > 0
            ? unionRect(localMonitors)
            : primary.bounds;
        const updatedPrimary: ComputerNode = {
          ...primary,
          id: status.local_id || primary.id,
          name: status.local_name
            ? `${status.local_name} (This PC)`
            : primary.name,
          bounds: localUnion,
          monitors: localMonitors,
        };
        const updatedComputers = mergePeers(
          [updatedPrimary, ...currentTopology.computers.filter((c) => !c.is_local)],
          status.peers
        );
        const updatedLinks = pruneLinks(currentTopology.links, updatedComputers);
        const selectedStillPresent =
          state.selectedPeer &&
          updatedComputers.some(
            (c) => !c.is_local && c.id === state.selectedPeer?.id
          );

        return {
          status,
          error: null,
          selectedPeer: selectedStillPresent ? state.selectedPeer : null,
          topology: {
            computers: updatedComputers,
            links: updatedLinks,
          },
        };
      });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
    }
  },

  setAllowControl: async (enabled: boolean) => {
    saveAllowControl(enabled);
    set({ isLoading: true, error: null });
    try {
      const status = await kvmSetAllowControl(enabled);
      set({ status, isLoading: false });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), isLoading: false });
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

  refreshTrusted: async () => {
    try {
      const trustedPeers = await kvmListTrusted();
      set({ trustedPeers });
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
      const peerOrigin = computePeerOrigin(primary.bounds, peerUnion, savedPlacement, 0);

      const peerNode: ComputerNode = {
        id: peerId,
        name: peerName,
        is_local: false,
        address: peerIp,
        bounds: {
          x: peerOrigin.x,
          y: peerOrigin.y,
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
            to_edge: oppositeEdge(savedPlacement),
            offset_x: 0,
            offset_y: 0,
            deadzone_px: settings.cornerDeadzonePx,
            resistance_ms: settings.edgeResistanceMs,
          },
        ],
      };

      await kvmUpdateTopology(nextTopology);
      // Re-read status so controller_edge reflects the capture just started.
      const afterStatus = await kvmGetStatus().catch(() => status);
      set({
        status: afterStatus,
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
      if (currentTopology.links.length > 0) {
        await kvmUpdateTopology(resetTopology).catch(() => {});
      }
      const freshStatus = await kvmGetStatus().catch(() => null);
      set((state) => ({
        status: freshStatus ?? {
          ...state.status,
          linked_peer: null,
          controller_edge: null,
          is_input_trapped: false,
          receiver_entered: false,
          connected_peers: [],
          peers: [],
        },
        selectedPeer: null,
        topology: resetTopology,
        cursorPos: null,
        isLoading: false,
      }));
      logActivity({
        kind: "kvm",
        title: prevPeer ? `Unlinked ${prevPeer.name}` : "Unlinked PC",
      });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), isLoading: false });
    }
  },

  forgetPeer: async (computerId: string) => {
    set({ isLoading: true, error: null });
    try {
      await kvmForgetPeer(computerId);
      const [freshStatus, trustedPeers] = await Promise.all([
        kvmGetStatus().catch(() => null),
        kvmListTrusted().catch(() => get().trustedPeers),
      ]);
      set((state) => ({
        ...(freshStatus ? { status: freshStatus } : {}),
        trustedPeers,
        selectedPeer: state.selectedPeer?.id === computerId ? null : state.selectedPeer,
        isLoading: false,
      }));
      await get().refreshStatus();
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e), isLoading: false });
    }
  },

  setTopology: async (topology: TopologyConfig) => {
    set({ topology });
    if (topology.links[0]?.from_edge) {
      savePlacement(topology.links[0].from_edge);
    }
    try {
      await kvmUpdateTopology(topology);
      const status = await kvmGetStatus().catch(() => null);
      if (status) {
        set({ status });
      }
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

    if (
      partial.syncClipboardText !== undefined ||
      partial.syncClipboardImage !== undefined ||
      partial.syncFileTransfer !== undefined
    ) {
      void kvmSetSyncSettings(
        next.syncClipboardText,
        next.syncClipboardImage,
        next.syncFileTransfer
      ).catch(() => {});
    }

    if (partial.debugLog !== undefined) {
      void kvmSetDebugLog(Boolean(next.debugLog)).catch(() => {});
    }

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
      trustedPeers: [],
      topology: defaultTopology,
      selectedPeer: null,
      isPairingModalOpen: false,
      isLoading: false,
      error: null,
      activityFeed: [],
      cursorPos: null,
    }),
}));
