import { invoke } from "@tauri-apps/api/core";
import type { DiscoveredPeer, KvmStatus, TopologyConfig } from "@/types/kvm";

export async function kvmGetStatus(): Promise<KvmStatus> {
  return await invoke<KvmStatus>("kvm_get_status");
}

export async function kvmStartServer(
  tcpPort = 4177,
  udpPort = 4178
): Promise<KvmStatus> {
  return await invoke<KvmStatus>("kvm_start_server", {
    tcpPort,
    udpPort,
  });
}

export async function kvmStopServer(): Promise<void> {
  return await invoke<void>("kvm_stop_server");
}

export async function kvmGetDiscoveredPeers(): Promise<DiscoveredPeer[]> {
  return await invoke<DiscoveredPeer[]>("kvm_get_discovered_peers");
}

export async function kvmConnectPeer(
  ip: string,
  tcpPort = 4177,
  udpPort = 4178
): Promise<void> {
  return await invoke<void>("kvm_connect_peer", {
    ip,
    tcpPort,
    udpPort,
  });
}

export async function kvmDisconnectPeer(): Promise<void> {
  return await invoke<void>("kvm_disconnect_peer");
}

export async function kvmUpdateTopology(
  topology: TopologyConfig
): Promise<void> {
  return await invoke<void>("kvm_update_topology", {
    topology,
  });
}

export async function kvmEmergencyRelease(): Promise<void> {
  return await invoke<void>("kvm_emergency_release");
}

export async function kvmSendFile(filePath: string): Promise<void> {
  return await invoke<void>("kvm_send_file", { filePath });
}
