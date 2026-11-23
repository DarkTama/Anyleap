import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EmbeddedMirrorWindow } from "./EmbeddedMirrorWindow";
import { useAppStore } from "@/store/useAppStore";
import type { SessionInfo } from "@/lib/types";

let startedHandler: ((e: { payload: SessionInfo }) => void) | null = null;
let replacedHandler: ((e: { payload: string }) => void) | null = null;
let clipboardResultHandler:
  | ((e: { payload: { serial: string; dest?: string; error?: string } }) => void)
  | null = null;
const mockListSessions = vi.fn<() => Promise<SessionInfo[]>>();

const mockWin = {
  label: "mirror-test",
  destroy: vi.fn(() => Promise.resolve()),
  close: vi.fn(() => Promise.resolve()),
  minimize: vi.fn(() => Promise.resolve()),
  toggleMaximize: vi.fn(() => Promise.resolve()),
  startDragging: vi.fn(() => Promise.resolve()),
  onCloseRequested: vi.fn(() => Promise.resolve(() => {})),
};

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => mockWin,
}));

vi.mock("@/lib/tauri", async (importOriginal) => {
  const orig = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...orig,
    embedMirror: vi.fn(() => Promise.resolve()),
    resizeEmbeddedMirror: vi.fn(() => Promise.resolve()),
    pushClipboardImage: vi.fn(() => Promise.resolve()),
    stopMirror: vi.fn(() => Promise.resolve()),
    listSessions: () => mockListSessions(),
    onSessionStarted: vi.fn((cb: (e: { payload: SessionInfo }) => void) => {
      startedHandler = cb;
      return Promise.resolve(() => {});
    }),
    onSessionReplaced: vi.fn((cb: (e: { payload: string }) => void) => {
      replacedHandler = cb;
      return Promise.resolve(() => {});
    }),
    onSessionExited: vi.fn(() => Promise.resolve(() => {})),
    onMirrorClipboardImageResult: vi.fn(
      (cb: (e: { payload: { serial: string; dest?: string; error?: string } }) => void) => {
        clipboardResultHandler = cb;
        return Promise.resolve(() => {});
      },
    ),
    getPhoneToggles: vi.fn(() => Promise.resolve({ wifi: false, bluetooth: false, dnd: false })),
    getCallActive: vi.fn(() => Promise.resolve(false)),
  };
});

describe("EmbeddedMirrorWindow", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    startedHandler = null;
    useAppStore.getState().setSessions([]);
    mockListSessions.mockReset();
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    // Stub ResizeObserver for JSDOM
    globalThis.ResizeObserver = class {
      observe() {}
      disconnect() {}
      unobserve() {}
    } as unknown as typeof ResizeObserver;
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    useAppStore.getState().setSessions([]);
  });

  it("syncs active recording session into useAppStore and hides Rotate in flexDisplay", async () => {
    const baseSession: SessionInfo = {
      id: "sess-1",
      serial: "dev-1",
      pid: 100,
      started_at: 1,
      mode: "display",
      recording_path: null,
    };
    mockListSessions.mockResolvedValue([baseSession]);

    await act(async () => {
      root.render(<EmbeddedMirrorWindow serial="dev-1" initialMode="display" flexDisplay={true} />);
    });

    expect(useAppStore.getState().sessions).toHaveLength(1);
    expect(host.querySelector('button[aria-label="Rotate"]')).toBeNull();
    expect(host.querySelector('button[aria-label="Record screen"]')).not.toBeNull();

    await act(async () => {
      replacedHandler?.({ payload: "sess-1" });
      startedHandler?.({
        payload: { ...baseSession, id: "sess-2", recording_path: "C:/Videos/rec.mp4" },
      });
    });

    expect(host.querySelector('button[aria-label="Stop recording"]')).not.toBeNull();

    await act(async () => {
      replacedHandler?.({ payload: "sess-2" });
      startedHandler?.({
        payload: { ...baseSession, id: "sess-3", recording_path: null },
      });
    });

    expect(host.querySelector('button[aria-label="Record screen"]')).not.toBeNull();
  });

  it("shows Rotate in Mirror mode (flexDisplay=false)", async () => {
    mockListSessions.mockResolvedValue([
      { id: "sess-3", serial: "dev-2", pid: 101, started_at: 1, mode: "display", recording_path: null },
    ]);

    await act(async () => {
      root.render(<EmbeddedMirrorWindow serial="dev-2" initialMode="display" flexDisplay={false} />);
    });

    expect(host.querySelector('button[aria-label="Rotate"]')).not.toBeNull();
  });

  it("shows toast when backend Ctrl+V image paste completes", async () => {
    mockListSessions.mockResolvedValue([
      { id: "sess-4", serial: "dev-3", pid: 102, started_at: 1, mode: "display", recording_path: null },
    ]);

    await act(async () => {
      root.render(<EmbeddedMirrorWindow serial="dev-3" initialMode="display" flexDisplay={false} />);
    });

    await act(async () => {
      clipboardResultHandler?.({
        payload: { serial: "dev-3", dest: "/sdcard/Download/paste_123.png" },
      });
    });

    expect(host.textContent).toContain("Image pushed to /sdcard/Download/paste_123.png");
  });
});
