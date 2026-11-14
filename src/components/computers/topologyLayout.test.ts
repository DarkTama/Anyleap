import { describe, it, expect } from "vitest";
import {
  computeCanvasLayout,
  computePresetOrigin,
  computeSeamCanvasRects,
  computeSeams,
  getComputerRole,
  getVisibleSections,
  hasMonitorOverlap,
  resolveRemoteOrigin,
  snapRemoteOrigin,
} from "./topologyLayout";
import type { KvmStatus, MonitorDisplay } from "@/types/kvm";

function makeStatus(overrides: Partial<KvmStatus> = {}): KvmStatus {
  return {
    is_host_running: false,
    allow_control: false,
    port: 4177,
    security_pin: "",
    connected_peers: [],
    is_input_trapped: false,
    linked_peer: null,
    ...overrides,
  };
}

describe("getComputerRole & getVisibleSections", () => {
  it("returns 'none' and shows allow-control, pairing code, and discovered peers when not linked", () => {
    const role = getComputerRole(makeStatus());
    expect(role).toBe("none");

    const sections = getVisibleSections(role);
    expect(sections.showStatusCard).toBe(true);
    expect(sections.showAllowControlToggle).toBe(true);
    expect(sections.showPairingCode).toBe(true);
    expect(sections.showDiscoveredPeers).toBe(true);
    expect(sections.showLayoutCanvas).toBe(false);
    expect(sections.showBringCursorBack).toBe(false);
    expect(sections.showControlledByCard).toBe(false);
    expect(sections.showTrustedPeers).toBe(false);
    expect(sections.showUnlink).toBe(false);
  });

  it("returns 'main' and shows layout canvas, edge controls, unlink, and bring cursor back", () => {
    const role = getComputerRole(
      makeStatus({
        linked_peer: { computer_id: "peer-1", name: "Laptop", role: "main" },
        controller_edge: "right",
      })
    );
    expect(role).toBe("main");

    const sections = getVisibleSections(role);
    expect(sections.showLayoutCanvas).toBe(true);
    expect(sections.showEdgeControls).toBe(true);
    expect(sections.showBringCursorBack).toBe(true);
    expect(sections.showUnlink).toBe(true);
    expect(sections.showAllowControlToggle).toBe(false);
    expect(sections.showPairingCode).toBe(false);
    expect(sections.showDiscoveredPeers).toBe(false);
    expect(sections.showControlledByCard).toBe(false);
  });

  it("returns 'linked' and hides canvas and controller-only controls", () => {
    const role = getComputerRole(
      makeStatus({
        allow_control: true,
        is_host_running: true,
        linked_peer: { computer_id: "main-1", name: "Desktop", role: "linked" },
      })
    );
    expect(role).toBe("linked");

    const sections = getVisibleSections(role);
    expect(sections.showControlledByCard).toBe(true);
    expect(sections.showTrustedPeers).toBe(true);
    expect(sections.showUnlink).toBe(true);
    expect(sections.showLayoutCanvas).toBe(false);
    expect(sections.showEdgeControls).toBe(false);
    expect(sections.showBringCursorBack).toBe(false);
    expect(sections.showDiscoveredPeers).toBe(false);
  });
});

describe("computeCanvasLayout", () => {
  const localNode = {
    bounds: { x: 0, y: 0, width: 1920, height: 1080 },
    monitors: [
      { id: "m1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
    ],
  };

  const peerNode = {
    bounds: { x: 0, y: 0, width: 2560, height: 1440 },
    monitors: [
      { id: "p1", x: 0, y: 0, width: 2560, height: 1440, is_primary: true },
    ],
  };

  it("scales both PCs proportionally and places peer on the right with offset_y", () => {
    const res = computeCanvasLayout({
      localNode,
      peerNode,
      edge: "right",
      offsetPx: 200,
      canvasWidth: 800,
      canvasHeight: 300,
      padding: 20,
    });

    // Total width = 1920 + 2560 = 4480, total height = 200 + 1440 = 1640
    // availW = 760, availH = 260 -> scale = min(760/4480, 260/1640) = 260/1640
    expect(res.scale).toBeCloseTo(260 / 1640, 5);
    expect(res.peerUnionPhysical.x).toBe(1920);
    expect(res.peerUnionPhysical.y).toBe(200);
    // Peer canvas right edge touches local canvas right edge at x = 1920
    expect(res.peerUnionCanvas.left).toBeCloseTo(
      res.localUnionCanvas.left + res.localUnionCanvas.width,
      4
    );
    // Local 1920x1080 vs Peer 2560x1440 maintain exact 1.333x ratio
    expect(res.peerMonitors[0].canvas.width / res.localMonitors[0].canvas.width).toBeCloseTo(
      2560 / 1920,
      4
    );
  });

  it("places peer on top with offset_x and preserves multi-monitor relative positions", () => {
    const multiPeer = {
      bounds: { x: -1280, y: 0, width: 3200, height: 1080 },
      monitors: [
        { id: "p-left", x: -1280, y: 0, width: 1280, height: 720, is_primary: false },
        { id: "p-right", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
      ],
    };

    const res = computeCanvasLayout({
      localNode,
      peerNode: multiPeer,
      edge: "top",
      offsetPx: -300,
      canvasWidth: 800,
      canvasHeight: 400,
      padding: 20,
    });

    expect(res.peerUnionPhysical).toEqual({
      x: -300,
      y: -1080,
      width: 3200,
      height: 1080,
    });
    expect(res.peerMonitors).toHaveLength(2);
    expect(res.peerMonitors[0].physical).toEqual({
      x: -300,
      y: -1080,
      width: 1280,
      height: 720,
    });
    expect(res.peerMonitors[1].physical).toEqual({
      x: 980,
      y: -1080,
      width: 1920,
      height: 1080,
    });
    // Bottom of peer union touches top of local union
    expect(res.peerUnionCanvas.top + res.peerUnionCanvas.height).toBeCloseTo(
      res.localUnionCanvas.top,
      4
    );
  });

  it("draws sharedEdgeSpans only where edge monitors of both PCs overlap after offset", () => {
    // Example layout: landscape 1920x1080 at (0,0) + portrait 1080x1920 at (1920,-420) on Main;
    // Linked 1920x1080 on the left.
    const mainMulti = {
      bounds: { x: 0, y: -420, width: 3000, height: 1920 },
      monitors: [
        { id: "landscape", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
        { id: "portrait", x: 1920, y: -420, width: 1080, height: 1920, is_primary: false },
      ],
    };
    const linkedSingle = {
      bounds: { x: 0, y: 0, width: 1920, height: 1080 },
      monitors: [
        { id: "peer", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
      ],
    };

    // With offset_y = 0, peerUnion starts at localUnion.y (-420), so peer spans [-420..660].
    // Only landscape [0..1080] touches Main's left edge, so overlap is [0..660].
    const res0 = computeCanvasLayout({
      localNode: mainMulti,
      peerNode: linkedSingle,
      edge: "left",
      offsetPx: 0,
      canvasWidth: 800,
      canvasHeight: 400,
      padding: 20,
    });
    expect(res0.sharedEdgeSpans).toHaveLength(1);
    expect(res0.sharedEdgeSpans[0].physicalStart).toBe(0);
    expect(res0.sharedEdgeSpans[0].physicalEnd).toBe(660);

    // With offset_y = 420, peer aligns with landscape [0..1080] -> full [0..1080].
    const resAligned = computeCanvasLayout({
      localNode: mainMulti,
      peerNode: linkedSingle,
      edge: "left",
      offsetPx: 420,
      canvasWidth: 800,
      canvasHeight: 400,
      padding: 20,
    });
    expect(resAligned.sharedEdgeSpans).toHaveLength(1);
    expect(resAligned.sharedEdgeSpans[0].physicalStart).toBe(0);
    expect(resAligned.sharedEdgeSpans[0].physicalEnd).toBe(1080);

    // Top-edge variant: Main has narrow top-edge monitor [0..1920, 0..1080] and wider non-edge [-400..2400, 1080..2160];
    // Peer has [0..1280, 0..720] placed on top with offset_x = 1000 -> overlap is [1000..1920].
    const topRes = computeCanvasLayout({
      localNode: {
        bounds: { x: -400, y: 0, width: 2800, height: 2160 },
        monitors: [
          { id: "top", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
          { id: "wide-below", x: -400, y: 1080, width: 2800, height: 1080, is_primary: false },
        ],
      },
      peerNode: {
        bounds: { x: 0, y: 0, width: 1280, height: 720 },
        monitors: [{ id: "p", x: 0, y: 0, width: 1280, height: 720, is_primary: true }],
      },
      edge: "top",
      offsetPx: 1400, // localUnion.x = -400 -> peer.x = 1000..2280
      canvasWidth: 800,
      canvasHeight: 400,
      padding: 20,
    });
    expect(topRes.sharedEdgeSpans).toHaveLength(1);
    expect(topRes.sharedEdgeSpans[0].physicalStart).toBe(1000);
    expect(topRes.sharedEdgeSpans[0].physicalEnd).toBe(1920);
  });
});

describe("SPEC-24 per-monitor seams, snapping, overlap rejection, and presets", () => {
  // Tama's layout: Main landscape 1920x1080 at (0,0) + portrait D1 at (1920,-420)
  const tamaMain: MonitorDisplay[] = [
    {
      id: "\\\\.\\DISPLAY1",
      device_name: "\\\\.\\DISPLAY1",
      x: 0,
      y: 0,
      width: 1920,
      height: 1080,
      is_primary: true,
    },
    {
      id: "\\\\.\\DISPLAY2",
      device_name: "\\\\.\\DISPLAY2",
      x: 1920,
      y: -420,
      width: 1080,
      height: 1920,
      is_primary: false,
    },
  ];

  const linked1200: MonitorDisplay[] = [
    {
      id: "\\\\.\\DISPLAY1",
      device_name: "\\\\.\\DISPLAY1",
      x: 0,
      y: 0,
      width: 1920,
      height: 1200,
      is_primary: true,
    },
  ];

  it("computeSeams handles Tama's layout: left, bottom under landscape (dual seam), and under both", () => {
    // 1) Left of Main landscape
    const leftSeams = computeSeams(tamaMain, linked1200, { x: -1920, y: 0 });
    expect(leftSeams).toEqual([
      {
        main_monitor_id: "\\\\.\\DISPLAY1",
        linked_monitor_id: "\\\\.\\DISPLAY1",
        side: "left",
        start: 0,
        end: 1080,
      },
    ]);

    // 2) Bottom under Main landscape: touches DISPLAY1 bottom [0..1920] AND DISPLAY2 left [1080..1500]
    const underLandscapeOrigin = { x: 0, y: 1080 };
    expect(hasMonitorOverlap(tamaMain, linked1200, underLandscapeOrigin)).toBe(false);
    const underLandscapeSeams = computeSeams(tamaMain, linked1200, underLandscapeOrigin);
    expect(underLandscapeSeams).toEqual([
      {
        main_monitor_id: "\\\\.\\DISPLAY1",
        linked_monitor_id: "\\\\.\\DISPLAY1",
        side: "bottom",
        start: 0,
        end: 1920,
      },
      {
        main_monitor_id: "\\\\.\\DISPLAY2",
        linked_monitor_id: "\\\\.\\DISPLAY1",
        side: "left",
        start: 1080,
        end: 1500,
      },
    ]);

    // 3) Under both landscape monitors: Main landscape at (0,0) + D1 landscape at (1920,-420 -> bottom 660; or side-by-side at y=0)
    const dualLandscapeMain: MonitorDisplay[] = [
      { id: "m1", device_name: "m1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
      { id: "m2", device_name: "m2", x: 1920, y: 0, width: 1920, height: 1080, is_primary: false },
    ];
    const underBothSeams = computeSeams(dualLandscapeMain, linked1200, { x: 960, y: 1080 });
    expect(underBothSeams).toEqual([
      { main_monitor_id: "m1", linked_monitor_id: "\\\\.\\DISPLAY1", side: "bottom", start: 960, end: 1920 },
      { main_monitor_id: "m2", linked_monitor_id: "\\\\.\\DISPLAY1", side: "bottom", start: 1920, end: 2880 },
    ]);

    // 4) Gap or corner-only touch produces 0 seams
    expect(computeSeams(tamaMain, linked1200, { x: -1921, y: 0 })).toEqual([]);
    expect(computeSeams(tamaMain, linked1200, { x: -1920, y: 1080 })).toEqual([]);
  });

  it("snapRemoteOrigin snaps within 24 screen px and rejects gaps > 24px and deep overlaps", () => {
    const scale = 0.2; // 24 screen px = 120 physical px

    // Within 24 screen px of right edge of D2 (x=3000): 3050 is 10 screen px away
    const snapRight = snapRemoteOrigin({
      mainMonitors: tamaMain,
      linkedMonitors: linked1200,
      candidateOrigin: { x: 3050, y: -200 },
      scale,
    });
    expect(snapRight.valid).toBe(true);
    expect(snapRight.snapped).toBe(true);
    expect(snapRight.overlaps).toBe(false);
    expect(snapRight.origin).toEqual({ x: 3000, y: -200 });
    expect(snapRight.seams).toHaveLength(1);
    expect(snapRight.seams[0].side).toBe("right");

    // Slight overlap (10 screen px inside D1 bottom): snaps cleanly back onto y=1080
    const snapOutOfOverlap = snapRemoteOrigin({
      mainMonitors: tamaMain,
      linkedMonitors: linked1200,
      candidateOrigin: { x: 0, y: 1030 }, // 50 physical px overlap = 10 screen px at scale 0.2
      scale,
    });
    expect(snapOutOfOverlap.valid).toBe(true);
    expect(snapOutOfOverlap.overlaps).toBe(false);
    expect(snapOutOfOverlap.origin).toEqual({ x: 0, y: 1080 });

    // Deep overlap (> 24 screen px inside Main landscape): rejected
    const deepOverlap = snapRemoteOrigin({
      mainMonitors: tamaMain,
      linkedMonitors: linked1200,
      candidateOrigin: { x: 400, y: 300 },
      scale: 1,
    });
    expect(deepOverlap.valid).toBe(false);
    expect(deepOverlap.overlaps).toBe(true);
    expect(deepOverlap.reason).toBe("overlap");
    expect(deepOverlap.seams).toEqual([]);

    // Far gap (> 24 screen px away): rejected
    const farGap = snapRemoteOrigin({
      mainMonitors: tamaMain,
      linkedMonitors: linked1200,
      candidateOrigin: { x: 3200, y: 0 }, // 200 * 0.2 = 40 screen px > 24px
      scale,
    });
    expect(farGap.valid).toBe(false);
    expect(farGap.overlaps).toBe(false);
    expect(farGap.reason).toBe("no-seam");
    expect(farGap.seams).toEqual([]);
  });

  it("snapRemoteOrigin works when Linked PC has monitors at negative coordinates", () => {
    const multiLinked: MonitorDisplay[] = [
      { id: "p-left", x: -1280, y: 0, width: 1280, height: 720, is_primary: false },
      { id: "p-main", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
    ];
    const mainSingle: MonitorDisplay[] = [
      { id: "m1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
    ];
    // Placing p-left (x=-1280) to touch m1 right (x=1920) requires remote_origin.x = 3200
    const snap = snapRemoteOrigin({
      mainMonitors: mainSingle,
      linkedMonitors: multiLinked,
      candidateOrigin: { x: 3210, y: 15 },
      scale: 0.2,
    });
    expect(snap.valid).toBe(true);
    expect(snap.origin).toEqual({ x: 3200, y: 0 });
    expect(snap.seams).toEqual([
      {
        main_monitor_id: "m1",
        linked_monitor_id: "p-left",
        side: "right",
        start: 0,
        end: 720,
      },
    ]);
  });

  it("computePresetOrigin always yields non-overlapping, seam-forming remote_origin on all 4 sides", () => {
    const multiLinked: MonitorDisplay[] = [
      { id: "p-left", x: -1280, y: 0, width: 1280, height: 720, is_primary: false },
      { id: "p-main", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
    ];

    for (const side of ["left", "right", "top", "bottom"] as const) {
      const origin = computePresetOrigin(tamaMain, multiLinked, side);
      expect(hasMonitorOverlap(tamaMain, multiLinked, origin)).toBe(false);
      const seams = computeSeams(tamaMain, multiLinked, origin);
      expect(seams.length).toBeGreaterThan(0);
      expect(seams.some((s) => s.side === side)).toBe(true);
    }
  });

  it("resolveRemoteOrigin prefers remote_origin and migrates SPEC-23 link edge/offset when absent", () => {
    const mainSingle: MonitorDisplay[] = [
      { id: "m1", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
    ];
    const linkedMulti: MonitorDisplay[] = [
      { id: "p-left", x: -1280, y: 0, width: 1280, height: 720, is_primary: false },
      { id: "p-main", x: 0, y: 0, width: 1920, height: 1080, is_primary: true },
    ];

    // Explicit remote_origin supersedes link
    expect(
      resolveRemoteOrigin({
        mainMonitors: mainSingle,
        linkedMonitors: linkedMulti,
        remoteOrigin: { x: 100, y: 1080 },
        link: { from_edge: "right", offset_x: 0, offset_y: 250 },
      })
    ).toEqual({ x: 100, y: 1080 });

    // Migration from SPEC-23 link: right edge, offset_y = 200 -> unionOrigin = (1920, 200) -> remoteOrigin = (3200, 200)
    expect(
      resolveRemoteOrigin({
        mainMonitors: mainSingle,
        linkedMonitors: linkedMulti,
        remoteOrigin: null,
        link: { from_edge: "right", offset_x: 0, offset_y: 200 },
      })
    ).toEqual({ x: 3200, y: 200 });
  });

  it("computeSeamCanvasRects maps multi-side seams onto exact canvas monitor boundaries", () => {
    const layout = computeCanvasLayout({
      localNode: {
        bounds: { x: 0, y: -420, width: 3000, height: 1920 },
        monitors: tamaMain,
      },
      peerNode: {
        bounds: { x: 0, y: 0, width: 1920, height: 1200 },
        monitors: linked1200,
      },
      remoteOrigin: { x: 0, y: 1080 },
      canvasWidth: 800,
      canvasHeight: 400,
      padding: 20,
    });

    expect(layout.seams).toHaveLength(2);
    expect(layout.seamRects).toHaveLength(2);

    const bottomRect = layout.seamRects.find((r) => r.seam.side === "bottom")!;
    const leftRect = layout.seamRects.find((r) => r.seam.side === "left")!;

    expect(bottomRect.orientation).toBe("horizontal");
    expect(bottomRect.canvas.top + bottomRect.canvas.height / 2).toBeCloseTo(
      layout.localMonitors[0].canvas.top + layout.localMonitors[0].canvas.height,
      4
    );
    expect(bottomRect.canvas.width).toBeCloseTo(1920 * layout.scale, 4);

    expect(leftRect.orientation).toBe("vertical");
    expect(leftRect.canvas.left + leftRect.canvas.width / 2).toBeCloseTo(
      layout.localMonitors[1].canvas.left,
      4
    );
    expect(leftRect.canvas.height).toBeCloseTo((1500 - 1080) * layout.scale, 4);

    // Direct call to computeSeamCanvasRects matches layout.seamRects
    const direct = computeSeamCanvasRects({
      seams: layout.seams,
      localMonitors: layout.localMonitors,
      viewport: layout.viewport,
    });
    expect(direct).toEqual(layout.seamRects);
  });
});
