import { describe, it, expect, beforeEach, vi } from "vitest";
import { useKvmStore } from "../kvmStore";
import * as kvmService from "@/services/kvmService";

describe("kvmStore", () => {
  beforeEach(() => {
    useKvmStore.getState().reset();
    vi.restoreAllMocks();
  });

  it("initializes with default status and host offline", () => {
    const state = useKvmStore.getState();
    expect(state.status.is_host_running).toBe(false);
    expect(state.discoveredPeers).toHaveLength(0);
    expect(state.topology.computers).toHaveLength(1);
    expect(state.topology.computers[0].is_local).toBe(true);
  });

  it("handles startServer success", async () => {
    vi.spyOn(kvmService, "kvmStartServer").mockResolvedValue({
      is_host_running: true,
      tcp_port: 4177,
      udp_port: 4178,
      security_pin: "123456",
      connected_peers: [],
      is_input_trapped: false,
    });

    await useKvmStore.getState().startServer();
    const state = useKvmStore.getState();
    expect(state.status.is_host_running).toBe(true);
    expect(state.status.security_pin).toBe("123456");
  });

  it("connectPeer updates topology and links peer to right edge", async () => {
    vi.spyOn(kvmService, "kvmConnectPeer").mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmUpdateTopology").mockResolvedValue(undefined);

    const peer = {
      id: "laptop-1",
      name: "Office Laptop",
      ip: "192.168.1.50",
      tcp_port: 4177,
      udp_port: 4178,
    };

    await useKvmStore.getState().connectPeer(peer);
    const state = useKvmStore.getState();
    expect(state.selectedPeer).toEqual(peer);
    expect(state.topology.computers).toHaveLength(2);
    expect(state.topology.computers[1].id).toBe("laptop-1");
    expect(state.topology.links).toHaveLength(1);
    expect(state.topology.links[0].from_edge).toBe("right");
  });
});
