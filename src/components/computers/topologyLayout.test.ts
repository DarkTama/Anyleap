import { describe, it, expect } from "vitest";
import {
  computeCanvasLayout,
  getComputerRole,
  getVisibleSections,
} from "./topologyLayout";
import type { KvmStatus } from "@/types/kvm";

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
