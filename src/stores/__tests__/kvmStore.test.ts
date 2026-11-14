import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  parseSavedSettings,
  reduceAudioState,
  reduceContinuityNotice,
  reduceTransferProgress,
  removeTransferProgress,
  useKvmStore,
} from "../kvmStore";
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
      port: 4177,
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
      port: 4177,
    };

    await useKvmStore.getState().connectPeer(peer);
    const state = useKvmStore.getState();
    expect(state.selectedPeer?.id).toBe("laptop-1");
    expect(state.topology.computers).toHaveLength(2);
    expect(state.topology.computers[1].id).toBe("laptop-1");
    expect(state.topology.links).toHaveLength(1);
    expect(state.topology.links[0].from_edge).toBe("right");
  });

  it("connectPeer uses received peer monitor layout union instead of 1920x1080 defaults", async () => {
    vi.spyOn(kvmService, "kvmConnectPeer").mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmUpdateTopology").mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValue({
      is_host_running: true,
      port: 4177,
      security_pin: "111222",
      connected_peers: [],
      is_input_trapped: false,
      local_id: "0123456789abcdef",
      local_name: "Desktop",
      monitors: [{ id: "mon-1", x: 0, y: 0, width: 2560, height: 1440, is_primary: true }],
      peers: [
        {
          id: "fedcba9876543210",
          name: "Workstation",
          is_local: false,
          address: "192.168.1.60",
          bounds: { x: 0, y: 0, width: 3840, height: 1200 },
          monitors: [
            { id: "mon-1", x: 0, y: 0, width: 1920, height: 1200, is_primary: true },
            { id: "mon-2", x: 1920, y: 0, width: 1920, height: 1080, is_primary: false },
          ],
        },
      ],
    });

    await useKvmStore.getState().connectPeer({
      id: "peer-192-168-1-60",
      name: "PC (192.168.1.60)",
      ip: "192.168.1.60",
      port: 4177,
    });

    const state = useKvmStore.getState();
    const remote = state.topology.computers.find((c) => !c.is_local);
    expect(remote?.id).toBe("fedcba9876543210");
    expect(remote?.bounds.width).toBe(3840);
    expect(remote?.bounds.height).toBe(1200);
    expect(remote?.monitors).toHaveLength(2);
  });

  it("refreshStatus merges receiver peers with their received bounds", async () => {
    vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValue({
      is_host_running: true,
      port: 4177,
      security_pin: "654321",
      connected_peers: ["Laptop"],
      is_input_trapped: false,
      peers: [
        {
          id: "aaaa1111bbbb2222",
          name: "Laptop",
          is_local: false,
          bounds: { x: 0, y: 0, width: 1600, height: 900 },
          monitors: [{ id: "mon-1", x: 0, y: 0, width: 1600, height: 900, is_primary: true }],
        },
      ],
    });

    await useKvmStore.getState().refreshStatus();
    const remote = useKvmStore.getState().topology.computers.find((c) => !c.is_local);
    expect(remote?.id).toBe("aaaa1111bbbb2222");
    expect(remote?.bounds).toEqual({ x: 0, y: 0, width: 1600, height: 900 });
  });

  it("refreshStatus prunes remote peers and links absent from status.peers (M1)", async () => {
    vi.spyOn(kvmService, "kvmConnectPeer").mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmUpdateTopology").mockResolvedValue(undefined);

    await useKvmStore.getState().connectPeer({
      id: "stale-peer",
      name: "Old Laptop",
      ip: "192.168.1.99",
      port: 4177,
    });
    expect(useKvmStore.getState().topology.computers).toHaveLength(2);
    expect(useKvmStore.getState().topology.links).toHaveLength(1);

    vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValue({
      is_host_running: true,
      allow_control: true,
      port: 4177,
      security_pin: "123456",
      connected_peers: [],
      is_input_trapped: false,
      linked_peer: null,
      peers: [],
    });

    await useKvmStore.getState().refreshStatus();
    const state = useKvmStore.getState();
    expect(state.topology.computers).toHaveLength(1);
    expect(state.topology.computers[0].is_local).toBe(true);
    expect(state.topology.links).toHaveLength(0);
    expect(state.selectedPeer).toBeNull();
  });

  it("disconnectPeer (Unlink) refreshes status from backend (H3)", async () => {
    vi.spyOn(kvmService, "kvmDisconnectPeer").mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmUpdateTopology").mockResolvedValue(undefined);
    const getStatusSpy = vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValue({
      is_host_running: true,
      allow_control: true,
      port: 4177,
      security_pin: "123456",
      connected_peers: [],
      is_input_trapped: false,
      controller_edge: null,
      linked_peer: null,
      peers: [],
    });

    await useKvmStore.getState().disconnectPeer();
    expect(getStatusSpy).toHaveBeenCalled();
    expect(useKvmStore.getState().status.linked_peer).toBeNull();
    expect(useKvmStore.getState().status.controller_edge).toBeNull();
  });

  it("updateSettings immediately calls kvmSetSyncSettings when sync toggles change (L1)", () => {
    const syncSpy = vi.spyOn(kvmService, "kvmSetSyncSettings").mockResolvedValue(undefined);
    useKvmStore.getState().updateSettings({ syncClipboardText: false });
    expect(syncSpy).toHaveBeenCalledWith(false, true, true);
  });

  it("debugLog defaults to false and updateSettings calls kvmSetDebugLog", () => {
    expect(useKvmStore.getState().settings.debugLog).toBe(false);
    const dbgSpy = vi
      .spyOn(kvmService, "kvmSetDebugLog")
      .mockResolvedValue("C:\\logs\\kvm-debug.log");
    useKvmStore.getState().updateSettings({ debugLog: true });
    expect(useKvmStore.getState().settings.debugLog).toBe(true);
    expect(dbgSpy).toHaveBeenCalledWith(true);
  });

  it("connectPeer computes remote_origin and sends it via kvmUpdateTopology", async () => {
    vi.spyOn(kvmService, "kvmConnectPeer").mockResolvedValue(undefined);
    const updateTopoSpy = vi
      .spyOn(kvmService, "kvmUpdateTopology")
      .mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValue({
      is_host_running: true,
      port: 4177,
      security_pin: "123456",
      connected_peers: ["Workstation"],
      is_input_trapped: false,
      local_id: "main-pc",
      local_name: "Main",
      monitors: [
        { id: "\\\\.\\DISPLAY1", device_name: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
      ],
      peers: [
        {
          id: "peer-pc",
          name: "Workstation",
          is_local: false,
          address: "192.168.1.60",
          bounds: { x: 0, y: 0, width: 1920, height: 1200 },
          monitors: [
            { id: "\\\\.\\DISPLAY1", device_name: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 1920, height: 1200, is_primary: true },
          ],
        },
      ],
    });

    await useKvmStore.getState().connectPeer({
      id: "peer-pc",
      name: "Workstation",
      ip: "192.168.1.60",
      port: 4177,
    });

    const state = useKvmStore.getState();
    expect(state.topology.remote_origin).toEqual({ x: 1920, y: 0 });
    expect(updateTopoSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        remote_origin: { x: 1920, y: 0 },
      })
    );
  });

  it("refreshStatus falls back to preset on previously used side and shows notice when monitors change", async () => {
    vi.spyOn(kvmService, "kvmConnectPeer").mockResolvedValue(undefined);
    const updateTopoSpy = vi
      .spyOn(kvmService, "kvmUpdateTopology")
      .mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValueOnce({
      is_host_running: true,
      port: 4177,
      security_pin: "123456",
      connected_peers: ["Laptop"],
      is_input_trapped: false,
      local_id: "main-pc",
      local_name: "Main",
      monitors: [
        { id: "\\\\.\\DISPLAY1", device_name: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
      ],
      peers: [
        {
          id: "peer-1",
          name: "Laptop",
          is_local: false,
          address: "192.168.1.77",
          bounds: { x: 0, y: 0, width: 1920, height: 1080 },
          monitors: [
            { id: "\\\\.\\DISPLAY1", device_name: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
          ],
        },
      ],
    }).mockResolvedValueOnce({
      is_host_running: true,
      port: 4177,
      security_pin: "123456",
      connected_peers: ["Laptop"],
      is_input_trapped: false,
      local_id: "main-pc",
      local_name: "Main",
      monitors: [
        { id: "\\\\.\\DISPLAY1", device_name: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
      ],
      peers: [
        {
          id: "peer-1",
          name: "Laptop",
          is_local: false,
          address: "192.168.1.77",
          bounds: { x: 0, y: 0, width: 1920, height: 1080 },
          monitors: [
            { id: "\\\\.\\DISPLAY1", device_name: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
          ],
        },
      ],
    });

    await useKvmStore.getState().connectPeer({
      id: "peer-1",
      name: "Laptop",
      ip: "192.168.1.77",
      port: 4177,
    });

    // Move peer to bottom via setTopology
    const cur = useKvmStore.getState().topology;
    await useKvmStore.getState().setTopology({
      ...cur,
      links: [{ ...cur.links[0], from_edge: "bottom", to_edge: "top" }],
      remote_origin: { x: 200, y: 1080 },
    });

    // Now Main PC gains a second monitor on refreshStatus
    vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValueOnce({
      is_host_running: true,
      port: 4177,
      security_pin: "123456",
      connected_peers: ["Laptop"],
      is_input_trapped: false,
      local_id: "main-pc",
      local_name: "Main",
      monitors: [
        { id: "\\\\.\\DISPLAY1", device_name: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
        { id: "\\\\.\\DISPLAY2", device_name: "\\\\.\\DISPLAY2", x: 1920, y: 0, width: 1920, height: 1080, is_primary: false },
      ],
      peers: [
        {
          id: "peer-1",
          name: "Laptop",
          is_local: false,
          address: "192.168.1.77",
          bounds: { x: 0, y: 0, width: 1920, height: 1080 },
          monitors: [
            { id: "\\\\.\\DISPLAY1", device_name: "\\\\.\\DISPLAY1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
          ],
        },
      ],
    });

    updateTopoSpy.mockClear();
    await useKvmStore.getState().refreshStatus();

    const after = useKvmStore.getState();
    expect(after.monitorChangeNotice).toMatch(/Main PC.*BOTTOM preset/i);
    expect(after.topology.remote_origin).toEqual({ x: 0, y: 1080 });
    expect(after.topology.links[0].from_edge).toBe("bottom");
    expect(updateTopoSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        remote_origin: { x: 0, y: 1080 },
      })
    );

    after.dismissMonitorChangeNotice();
    expect(useKvmStore.getState().monitorChangeNotice).toBeNull();
  });

  it("persists continuity settings and dispatches per-part commands on startup and change", () => {
    const filesSpy = vi.spyOn(kvmService, "kvmSetFilesSettings").mockResolvedValue(undefined);
    const audioSpy = vi.spyOn(kvmService, "kvmSetAudioSettings").mockResolvedValue(undefined);
    const sessionSpy = vi.spyOn(kvmService, "kvmSetSessionSettings").mockResolvedValue(undefined);

    useKvmStore.getState().syncContinuitySettings();
    expect(filesSpy).toHaveBeenCalledWith(false, true, 2048, false, 100);
    expect(audioSpy).toHaveBeenCalledWith(false, 100, true, true);
    expect(sessionSpy).toHaveBeenCalledWith(true, false, true);

    filesSpy.mockClear();
    audioSpy.mockClear();
    sessionSpy.mockClear();

    useKvmStore.getState().updateSettings({
      continuityPointerScheme: true,
      continuityClipPrefetchLimitMb: 250,
    });
    expect(filesSpy).toHaveBeenCalledWith(false, true, 2048, true, 250);
    expect(audioSpy).not.toHaveBeenCalled();
    expect(sessionSpy).not.toHaveBeenCalled();

    filesSpy.mockClear();
    useKvmStore.getState().updateSettings({
      continuityAudioBridge: true,
      continuityAudioVolume: 65,
      continuityMuteLinkedWhileBridging: false,
    });
    expect(audioSpy).toHaveBeenCalledWith(true, 65, true, false);
    expect(filesSpy).not.toHaveBeenCalled();
    expect(sessionSpy).not.toHaveBeenCalled();

    audioSpy.mockClear();
    useKvmStore.getState().updateSettings({ continuitySleepTogether: true });
    expect(sessionSpy).toHaveBeenCalledWith(true, true, true);
    expect(filesSpy).not.toHaveBeenCalled();
    expect(audioSpy).not.toHaveBeenCalled();

    const saved = JSON.parse(localStorage.getItem("anyleap_kvm_settings") || "{}");
    expect(saved.continuityPointerScheme).toBe(true);
    expect(saved.continuityClipPrefetchLimitMb).toBe(250);
    expect(saved.continuityAudioBridge).toBe(true);
    expect(saved.continuityAudioVolume).toBe(65);
    expect(saved.continuityMuteLinkedWhileBridging).toBe(false);
    expect(saved.continuitySleepTogether).toBe(true);
  });

  it("defaults continuityDragDropExperimental to false and ignores legacy continuityDragDrop key", () => {
    expect(useKvmStore.getState().settings.continuityDragDropExperimental).toBe(false);
    expect(parseSavedSettings(null).continuityDragDropExperimental).toBe(false);

    // Testers who previously persisted continuityDragDrop: true must migrate to false once
    const migrated = parseSavedSettings(
      JSON.stringify({ continuityDragDrop: true, continuityFileClipboard: true })
    );
    expect(migrated.continuityDragDropExperimental).toBe(false);
    expect("continuityDragDrop" in migrated).toBe(false);

    // Explicit opt-in under the new key is preserved
    const optedIn = parseSavedSettings(
      JSON.stringify({ continuityDragDrop: true, continuityDragDropExperimental: true })
    );
    expect(optedIn.continuityDragDropExperimental).toBe(true);

    const filesSpy = vi.spyOn(kvmService, "kvmSetFilesSettings").mockResolvedValue(undefined);
    useKvmStore.getState().updateSettings({ continuityDragDropExperimental: true });
    expect(filesSpy).toHaveBeenCalledWith(true, true, 2048, true, 250);
    const saved = JSON.parse(localStorage.getItem("anyleap_kvm_settings") || "{}");
    expect(saved.continuityDragDropExperimental).toBe(true);
  });

  it("reduces transfer progress, continuity notices, and audio state", () => {
    let transfers = reduceTransferProgress([], {
      id: "t1",
      kind: "drag",
      name: "notes.txt",
      pct: 25,
    });
    transfers = reduceTransferProgress(transfers, {
      id: "t2",
      kind: "clip",
      name: "photo.png",
      pct: 120,
    });
    transfers = reduceTransferProgress(transfers, {
      id: "t1",
      kind: "drag",
      name: "notes.txt",
      pct: -5,
    });
    expect(transfers).toEqual([
      { id: "t1", kind: "drag", name: "notes.txt", pct: 0 },
      { id: "t2", kind: "clip", name: "photo.png", pct: 100 },
    ]);
    expect(removeTransferProgress(transfers, "t1")).toEqual([
      { id: "t2", kind: "clip", name: "photo.png", pct: 100 },
    ]);

    expect(reduceContinuityNotice(null, { text: "   " }, 100)).toBeNull();
    expect(reduceContinuityNotice(null, { text: "  Saved to Downloads  " }, 1234)).toEqual({
      id: "notice-1234",
      text: "Saved to Downloads",
      timestamp: 1234,
    });

    expect(
      reduceAudioState(
        { streaming: false, level: 0, linkedMute: "unknown" },
        { streaming: true, level: 1.5, linkedMute: "unsupported" }
      )
    ).toEqual({ streaming: true, level: 1, linkedMute: "unsupported" });

    expect(
      reduceTransferProgress([], {
        id: "offer-1",
        kind: "clip",
        name: "archive.7z",
        pct: 50,
        state: "awaiting_fetch",
        sizeBytes: 204800,
      })
    ).toEqual([
      {
        id: "offer-1",
        kind: "clip",
        name: "archive.7z",
        pct: 0,
        state: "awaiting_fetch",
        sizeBytes: 204800,
      },
    ]);
  });
});
