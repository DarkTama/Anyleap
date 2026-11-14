import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TransferBarWindow } from "./TransferBarWindow";
import * as kvmService from "@/services/kvmService";
import { TRANSFER_CANCEL_FALLBACK_MS } from "./transferBarReducer";

const mockSetSize = vi.fn(() => Promise.resolve());
const mockSetPosition = vi.fn(() => Promise.resolve());
const mockClose = vi.fn(() => Promise.resolve());

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({
    setSize: mockSetSize,
    setPosition: mockSetPosition,
    close: mockClose,
  }),
}));

vi.mock("@tauri-apps/api/dpi", () => ({
  LogicalSize: class LogicalSize {
    width: number;
    height: number;
    constructor(width: number, height: number) {
      this.width = width;
      this.height = height;
    }
  },
  LogicalPosition: class LogicalPosition {
    x: number;
    y: number;
    constructor(x: number, y: number) {
      this.x = x;
      this.y = y;
    }
  },
}));

vi.mock("@/services/kvmService", () => ({
  kvmCancelTransfer: vi.fn(() => Promise.resolve()),
  kvmFetchClip: vi.fn(() => Promise.resolve()),
  listenKvmContinuityNotice: vi.fn(() => Promise.resolve(() => {})),
  listenKvmTransferProgress: vi.fn(() => Promise.resolve(() => {})),
}));

describe("TransferBarWindow hit area & sizing (SPEC-25)", () => {
  let container: HTMLDivElement;
  let root: Root;
  let resizeCallbacks: Array<() => void> = [];
  let mockRect = { width: 220.4, height: 32.2 };

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    mockSetSize.mockClear();
    mockSetPosition.mockClear();
    mockClose.mockClear();
    resizeCallbacks = [];
    mockRect = { width: 220.4, height: 32.2 };

    vi.stubGlobal(
      "ResizeObserver",
      class ResizeObserver {
        constructor(cb: () => void) {
          resizeCallbacks.push(cb);
        }
        observe() {}
        disconnect() {}
      },
    );

    Object.defineProperty(window, "screen", {
      configurable: true,
      value: { availWidth: 1920, availHeight: 1080 },
    });

    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      () =>
        ({
          width: mockRect.width,
          height: mockRect.height,
          top: 0,
          left: 0,
          bottom: mockRect.height,
          right: mockRect.width,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        }) as DOMRect,
    );

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    window.history.replaceState({}, "", "/");
  });

  it("sizes window to ceiled content bounding box without outer padding or external shadow, and bottom-centers", () => {
    const init = JSON.stringify({
      transfers: [
        {
          id: "t-1",
          kind: "clip",
          name: "screenshot.png",
          status: "awaiting_fetch",
          bytesDone: 0,
          sizeBytes: 2048,
          pct: 0,
        },
      ],
      notice: "Clipboard ready",
    });
    window.history.replaceState({}, "", `/?kvm_transfer_bar=1&init=${encodeURIComponent(init)}`);

    act(() => {
      root.render(<TransferBarWindow />);
    });

    const wrapper = container.firstElementChild as HTMLElement;
    expect(wrapper).not.toBeNull();
    // No outer padding, margin, or pointer-events-none wrapper
    expect(wrapper.className).not.toMatch(/\b(px-|py-|p-|mx-|my-|m-|pointer-events-none)\b/);
    // No external box-shadow outside pill bounds
    for (const child of Array.from(wrapper.children)) {
      expect((child as HTMLElement).className).not.toContain("shadow-elevated");
    }

    // 220.4 -> 221, 32.2 -> 33; x = round((1920 - 221)/2) = 850, y = 1080 - 33 - 24 = 1023
    expect(mockSetSize).toHaveBeenCalledWith(expect.objectContaining({ width: 221, height: 33 }));
    expect(mockSetPosition).toHaveBeenCalledWith(expect.objectContaining({ x: 850, y: 1023 }));

    // Simulate ResizeObserver firing when width changes (clamped to 340) and height changes
    mockRect = { width: 355.2, height: 68.7 };
    act(() => {
      for (const cb of resizeCallbacks) cb();
    });

    // Clamped width = 340, ceiled height = 69; x = round((1920 - 340)/2) = 790, y = 1080 - 69 - 24 = 987
    expect(mockSetSize).toHaveBeenLastCalledWith(expect.objectContaining({ width: 340, height: 69 }));
    expect(mockSetPosition).toHaveBeenLastCalledWith(expect.objectContaining({ x: 790, y: 987 }));
  });

  it("cancelling a row calls kvmCancelTransfer and removes it locally once confirmed", async () => {
    let resolveCancel!: () => void;
    const cancelSpy = vi.spyOn(kvmService, "kvmCancelTransfer").mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveCancel = resolve;
        }),
    );

    const init = JSON.stringify({
      transfers: [
        { id: "drag-1", kind: "drag", name: "video.mp4", pct: 40, state: "active" },
      ],
    });
    window.history.replaceState({}, "", `/?kvm_transfer_bar=1&init=${encodeURIComponent(init)}`);

    act(() => {
      root.render(<TransferBarWindow />);
    });

    const cancelBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent === "Cancel",
    );
    expect(cancelBtn).toBeDefined();

    await act(async () => {
      cancelBtn!.click();
    });
    expect(cancelSpy).toHaveBeenCalledWith("drag-1");
    expect(container.textContent).toContain("video.mp4");

    await act(async () => {
      resolveCancel();
      await Promise.resolve();
    });
    expect(container.textContent).not.toContain("video.mp4");
  });

  it("removes a cancelled row locally after 3s if kvmCancelTransfer has not confirmed", () => {
    vi.useFakeTimers();
    const cancelSpy = vi
      .spyOn(kvmService, "kvmCancelTransfer")
      .mockImplementation(() => new Promise<void>(() => {}));

    const init = JSON.stringify({
      transfers: [
        { id: "clip-1", kind: "clip", name: "dataset.zip", pct: 0, state: "awaiting_fetch" },
      ],
    });
    window.history.replaceState({}, "", `/?kvm_transfer_bar=1&init=${encodeURIComponent(init)}`);

    act(() => {
      root.render(<TransferBarWindow />);
    });

    const cancelBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent === "Cancel",
    );
    expect(cancelBtn).toBeDefined();

    act(() => {
      cancelBtn!.click();
    });
    expect(cancelSpy).toHaveBeenCalledWith("clip-1");
    expect(container.textContent).toContain("dataset.zip");

    act(() => {
      vi.advanceTimersByTime(TRANSFER_CANCEL_FALLBACK_MS);
    });
    expect(container.textContent).not.toContain("dataset.zip");
    vi.useRealTimers();
  });
});
