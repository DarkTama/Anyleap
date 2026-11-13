import { describe, it, expect, vi } from "vitest";
import {
  buildCommands,
  filterAndRankCommands,
  type PaletteCommand,
  type CommandPaletteContext,
} from "./commandPalette";

function makeCtx(overrides: Partial<CommandPaletteContext> = {}): CommandPaletteContext {
  return {
    selectedSerial: "ABC123",
    hasOnlineDevice: true,
    hasActiveSession: false,
    isRecording: false,
    kvmRunning: false,
    switchTab: vi.fn(),
    startMode: vi.fn(),
    stopMirroring: vi.fn(),
    toggleRecording: vi.fn(),
    openLinkOnPhone: vi.fn(),
    togglePhoneWifi: vi.fn(),
    togglePhoneBluetooth: vi.fn(),
    togglePhoneDnd: vi.fn(),
    takeScreenshot: vi.fn(),
    openPair: vi.fn(),
    openDoctor: vi.fn(),
    openTimeline: vi.fn(),
    checkForUpdates: vi.fn(),
    toggleKvmServer: vi.fn(),
    ...overrides,
  };
}

describe("filterAndRankCommands", () => {
  it("ranks label-prefix matches before label-substring and keyword matches", () => {
    const cmds: PaletteCommand[] = [
      { id: "kw", label: "Open diagnostics", group: "Test", keywords: ["pair"], run: () => {} },
      { id: "sub", label: "Start pair wizard", group: "Test", run: () => {} },
      { id: "pre", label: "Pair new device", group: "Test", run: () => {} },
    ];

    const res = filterAndRankCommands(cmds, "PAIR");
    expect(res.map((c) => c.id)).toEqual(["pre", "sub", "kw"]);
  });

  it("returns all commands on empty or whitespace query", () => {
    const cmds = buildCommands(makeCtx());
    expect(filterAndRankCommands(cmds, "   ")).toHaveLength(cmds.length);
  });

  it("marks device-dependent commands disabled with a reason when no device is selected", () => {
    const cmds = buildCommands(makeCtx({ selectedSerial: null, hasOnlineDevice: false }));
    const flex = cmds.find((c) => c.id === "mode-flex");
    const stop = cmds.find((c) => c.id === "stop-mirror");
    expect(flex?.disabled).toBe("No device selected");
    expect(stop?.disabled).toBe("No active session");
    expect(cmds.find((c) => c.id === "toggle-recording")?.disabled).toBe("No device selected");
    expect(cmds.find((c) => c.id === "open-link-on-phone")?.disabled).toBe("No device selected");
    expect(cmds.find((c) => c.id === "toggle-phone-wifi")?.disabled).toBe("No device selected");
    expect(cmds.find((c) => c.id === "toggle-phone-bluetooth")?.disabled).toBe("No device selected");
    expect(cmds.find((c) => c.id === "toggle-phone-dnd")?.disabled).toBe("No device selected");
  });

  it("toggles KVM server label based on kvmRunning", () => {
    const stopped = buildCommands(makeCtx({ kvmRunning: false }));
    const running = buildCommands(makeCtx({ kvmRunning: true }));
    expect(stopped.find((c) => c.id === "toggle-kvm")?.label).toBe("Start KVM server");
    expect(running.find((c) => c.id === "toggle-kvm")?.label).toBe("Stop KVM server");
  });
});
