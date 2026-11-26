import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EmbeddedMirrorWindow } from "./EmbeddedMirrorWindow";
import { useAppStore } from "@/store/useAppStore";
import { embedMirror } from "@/lib/tauri";
import { defaultModeDefaults } from "@/lib/modeSettings";
import type { SessionExited, SessionInfo } from "@/lib/types";

let startedHandler: ((e: { payload: SessionInfo }) => void) | null = null;
let restartingHandler: ((e: { payload: { id: string; serial: string; reason: string } }) => void) | null = null;
let replacedHandler: ((e: { payload: string }) => void) | null = null;
let exitedHandler: ((e: { payload: SessionExited }) => void) | null = null;
let clipboardResultHandler:
  | ((e: { payload: { serial: string; dest?: string; error?: string } }) => void)
  | null = null;
let scrollStepHandler: ((e: { payload: { serial: string; delta: 1 | -1 } }) => void) | null = null;
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
    onSessionRestarting: vi.fn(
      (cb: (e: { payload: { id: string; serial: string; reason: string } }) => void) => {
        restartingHandler = cb;
        return Promise.resolve(() => {});
      },
    ),
    onSessionReplaced: vi.fn((cb: (e: { payload: string }) => void) => {
      replacedHandler = cb;
      return Promise.resolve(() => {});
    }),
    onSessionExited: vi.fn((cb: (e: { payload: SessionExited }) => void) => {
      exitedHandler = cb;
      return Promise.resolve(() => {});
    }),
    onMirrorClipboardImageResult: vi.fn(
      (cb: (e: { payload: { serial: string; dest?: string; error?: string } }) => void) => {
        clipboardResultHandler = cb;
        return Promise.resolve(() => {});
      },
    ),
    onMirrorScrollStep: vi.fn(
      (cb: (e: { payload: { serial: string; delta: 1 | -1 } }) => void) => {
        scrollStepHandler = cb;
        return Promise.resolve(() => {});
      },
    ),
    getScrollMultiplier: vi.fn(() => Promise.resolve(2.75)),
    setScrollFeel: vi.fn(() => Promise.resolve(null)),
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
    restartingHandler = null;
    replacedHandler = null;
    exitedHandler = null;
    clipboardResultHandler = null;
    scrollStepHandler = null;
    mockWin.destroy.mockClear();
    mockWin.close.mockClear();
    vi.mocked(embedMirror).mockClear();
    useAppStore.setState({ modeDefaults: defaultModeDefaults(), sessions: [], sessionModes: {} });
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

  it("shows Reconnecting status during in-place restart and ignores stale session exits", async () => {
    const sess1: SessionInfo = {
      id: "sess-old",
      serial: "dev-4",
      pid: 200,
      started_at: 1,
      mode: "display",
      recording_path: null,
    };
    const sess2: SessionInfo = {
      ...sess1,
      id: "sess-new",
      pid: 201,
      started_at: 2,
    };
    mockListSessions.mockResolvedValue([sess1]);

    await act(async () => {
      root.render(<EmbeddedMirrorWindow serial="dev-4" initialMode="display" flexDisplay={true} />);
    });

    await act(async () => {
      restartingHandler?.({ payload: { id: "sess-old", serial: "dev-4", reason: "flex-idle" } });
    });
    expect(host.textContent).toContain("Reconnecting…");

    await act(async () => {
      replacedHandler?.({ payload: "sess-old" });
      startedHandler?.({ payload: sess2 });
    });
    expect(host.textContent).not.toContain("Reconnecting…");
    expect(embedMirror).toHaveBeenCalledWith("mirror-test", "dev-4");

    // A delayed session-exited from the old session must not destroy the active wrapper window.
    await act(async () => {
      exitedHandler?.({
        payload: { id: "sess-old", serial: "dev-4", code: 0, signal: null, last_error: "", stderr: "" },
      });
    });
    expect(mockWin.destroy).not.toHaveBeenCalled();
  });

  it("shows Scroll button in display mode, hides it in camera, and steps speed on Ctrl+Alt+-/+", async () => {
    mockListSessions.mockResolvedValue([
      { id: "sess-5", serial: "dev-5", pid: 300, started_at: 1, mode: "display", recording_path: null },
    ]);

    await act(async () => {
      root.render(
        <EmbeddedMirrorWindow serial="dev-5" initialMode="display" flexDisplay={true} displayMode="flex" />,
      );
    });

    expect(host.querySelector('button[aria-label="Scroll"]')).not.toBeNull();

    // Backend mirror-scroll-step event (Ctrl+Alt+- while scrcpy focused) steps Auto (2.75x) -> 2.5x
    await act(async () => {
      scrollStepHandler?.({ payload: { serial: "dev-5", delta: -1 } });
    });
    expect(useAppStore.getState().modeDefaults.flex.scroll.speed).toBe(2.5);
    expect(host.textContent).toContain("Scroll speed 2.5x · Flex");

    // Keyboard Ctrl+Alt+= while webview focused steps 2.5x -> 2.75x
    await act(async () => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "=", code: "Equal", ctrlKey: true, altKey: true, bubbles: true }),
      );
    });
    expect(useAppStore.getState().modeDefaults.flex.scroll.speed).toBe(2.75);
    expect(host.textContent).toContain("Scroll speed 2.75x · Flex");
  });

  it("hides Scroll button in Camera mode", async () => {
    mockListSessions.mockResolvedValue([
      { id: "sess-6", serial: "dev-cam", pid: 301, started_at: 1, mode: "camera", recording_path: null },
    ]);

    await act(async () => {
      root.render(
        <EmbeddedMirrorWindow serial="dev-cam" initialMode="camera" flexDisplay={false} />,
      );
    });
    expect(host.querySelector('button[aria-label="Scroll"]')).toBeNull();
  });
});
