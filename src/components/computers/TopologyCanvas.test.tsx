import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TopologyCanvas } from "./TopologyCanvas";
import { useKvmStore } from "@/stores/kvmStore";
import * as kvmService from "@/services/kvmService";

function dispatchPointer(
  target: Element,
  type: "pointerdown" | "pointermove" | "pointerup",
  coords: { clientX: number; clientY: number; pointerId?: number }
) {
  const PointerCtor =
    typeof PointerEvent !== "undefined" ? PointerEvent : MouseEvent;
  const evt = new PointerCtor(type, {
    bubbles: true,
    cancelable: true,
    clientX: coords.clientX,
    clientY: coords.clientY,
    button: 0,
    ...(typeof PointerEvent !== "undefined"
      ? { pointerId: coords.pointerId ?? 1, pointerType: "mouse" }
      : {}),
  });
  if (typeof (evt as unknown as { pointerId?: number }).pointerId !== "number") {
    Object.defineProperty(evt, "pointerId", { value: coords.pointerId ?? 1 });
  }
  target.dispatchEvent(evt);
}

describe("TopologyCanvas (SPEC-24)", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    localStorage.clear();
    useKvmStore.getState().reset();
    vi.restoreAllMocks();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    useKvmStore.setState({
      status: {
        is_host_running: false,
        port: 4177,
        security_pin: "",
        connected_peers: ["Laptop"],
        is_input_trapped: false,
        local_id: "main-pc",
        local_name: "MainPC",
        linked_peer: { computer_id: "peer-1", name: "Laptop", role: "main" },
        monitors: [
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
        ],
      },
      topology: {
        computers: [
          {
            id: "main-pc",
            name: "MainPC",
            is_local: true,
            bounds: { x: 0, y: -420, width: 3000, height: 1920 },
            monitors: [
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
            ],
          },
          {
            id: "peer-1",
            name: "Laptop",
            is_local: false,
            bounds: { x: 0, y: 1080, width: 1920, height: 1200 },
            monitors: [
              {
                id: "\\\\.\\DISPLAY1",
                device_name: "\\\\.\\DISPLAY1",
                x: 0,
                y: 0,
                width: 1920,
                height: 1200,
                is_primary: true,
              },
            ],
          },
        ],
        links: [
          {
            from_id: "main-pc",
            from_edge: "bottom",
            to_id: "peer-1",
            to_edge: "top",
            deadzone_px: 48,
            resistance_ms: 120,
          },
        ],
        remote_origin: { x: 0, y: 1080 },
      },
    });
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
  });

  it("renders per-monitor gold seam highlights and updates remote_origin via preset buttons", async () => {
    const updateTopoSpy = vi
      .spyOn(kvmService, "kvmUpdateTopology")
      .mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValue(
      useKvmStore.getState().status
    );

    await act(async () => {
      root.render(<TopologyCanvas />);
    });

    // At (0, 1080) under DISPLAY1, Tama's layout forms 2 seams: bottom + left
    const seams = container.querySelectorAll('[data-testid="seam-highlight"]');
    expect(seams).toHaveLength(2);
    expect(seams[0].getAttribute("data-seam-side")).toBe("bottom");
    expect(seams[1].getAttribute("data-seam-side")).toBe("left");

    const leftBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent === "Left"
    )!;
    await act(async () => {
      leftBtn.click();
    });

    expect(updateTopoSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        remote_origin: { x: -1920, y: -420 },
      })
    );
  });

  it("supports pointer-capture drag snapping and rejects overlapping drops", async () => {
    const updateTopoSpy = vi
      .spyOn(kvmService, "kvmUpdateTopology")
      .mockResolvedValue(undefined);
    vi.spyOn(kvmService, "kvmGetStatus").mockResolvedValue(
      useKvmStore.getState().status
    );

    await act(async () => {
      root.render(<TopologyCanvas />);
    });

    const linkedBlock = container.querySelector(
      '[data-testid="linked-pc-block"]'
    ) as HTMLDivElement;
    expect(linkedBlock).toBeTruthy();

    // 1) Drag deeply up into Main DISPLAY1 (overlap) and release -> rejected, no save
    await act(async () => {
      dispatchPointer(linkedBlock, "pointerdown", { clientX: 200, clientY: 200 });
    });
    // Transition must be disabled while dragging
    expect(linkedBlock.className).not.toContain("canvas-snap-transition");

    await act(async () => {
      dispatchPointer(linkedBlock, "pointermove", { clientX: 200, clientY: 120 });
    });
    expect(container.textContent).toContain("Overlaps Main PC");

    await act(async () => {
      dispatchPointer(linkedBlock, "pointerup", { clientX: 200, clientY: 120 });
    });
    expect(updateTopoSpy).not.toHaveBeenCalled();
    expect(linkedBlock.className).toContain("canvas-snap-transition");

    // 2) Drag slightly left (-12px screen -> within 24px snap to x=-100 or similar while keeping y=1080 seam)
    // Wait: at default canvasSize (760x320, padding 36 -> availW=688, availH=248; totalW=3000, totalH=2700 -> scale=248/2700=0.09185).
    // Moving clientX by -20px moves dxPhys = -218px, dyPhys = 5px (within 24px snap of y=1080!).
    await act(async () => {
      dispatchPointer(linkedBlock, "pointerdown", { clientX: 200, clientY: 200 });
      dispatchPointer(linkedBlock, "pointermove", { clientX: 180, clientY: 201 });
      dispatchPointer(linkedBlock, "pointerup", { clientX: 180, clientY: 201 });
    });

    expect(updateTopoSpy).toHaveBeenCalledTimes(1);
    const saved = updateTopoSpy.mock.calls[0][0];
    expect(saved.remote_origin?.y).toBe(1080);
    expect(saved.remote_origin?.x).toBeLessThan(0);
  });

  it("displays and dismisses the monitor-change notice", async () => {
    act(() => {
      useKvmStore.setState({
        monitorChangeNotice: "Display layout changed on Main PC — reset to BOTTOM preset.",
      });
    });

    await act(async () => {
      root.render(<TopologyCanvas />);
    });

    const notice = container.querySelector('[role="status"]');
    expect(notice?.textContent).toContain("Display layout changed on Main PC");

    const dismissBtn = container.querySelector(
      'button[aria-label="Dismiss monitor change notice"]'
    ) as HTMLButtonElement;
    await act(async () => {
      dismissBtn.click();
    });

    expect(container.querySelector('[role="status"]')).toBeNull();
  });
});
