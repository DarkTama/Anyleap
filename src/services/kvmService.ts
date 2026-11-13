import { invoke } from "@tauri-apps/api/core";
import type { DiscoveredPeer, KvmStatus, TopologyConfig } from "@/types/kvm";

export async function kvmGetStatus(): Promise<KvmStatus> {
  return await invoke<KvmStatus>("kvm_get_status");
}

export async function kvmStartServer(port = 4177): Promise<KvmStatus> {
  return await invoke<KvmStatus>("kvm_start_server", { port });
}

export async function kvmStopServer(): Promise<void> {
  return await invoke<void>("kvm_stop_server");
}

export async function kvmGetDiscoveredPeers(): Promise<DiscoveredPeer[]> {
  return await invoke<DiscoveredPeer[]>("kvm_get_discovered_peers");
}

export async function kvmConnectPeer(ip: string, port = 4177): Promise<void> {
  return await invoke<void>("kvm_connect_peer", { ip, port });
}

export async function kvmPairPeer(
  ip: string,
  port: number,
  pin: string
): Promise<void> {
  return await invoke<void>("kvm_pair_peer", { ip, port, pin });
}

export async function kvmForgetPeer(computerId: string): Promise<void> {
  return await invoke<void>("kvm_forget_peer", { computerId });
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

export async function kvmSetSyncSettings(
  text: boolean,
  image: boolean,
  files: boolean
): Promise<void> {
  return await invoke<void>("kvm_set_sync_settings", { text, image, files });
}
