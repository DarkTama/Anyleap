import { describe, it, expect, vi } from "vitest";
import {
  buildCommands,
  filterAndRankCommands,
  normalizeOpenableUrl,
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
    kvmHasPeers: true,
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
    openUrlOnLinkedPc: vi.fn(),
    toggleAudioBridge: vi.fn(),
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

  it("includes Open URL on Linked PC and Toggle audio bridge entries", () => {
    const openUrlOnLinkedPc = vi.fn();
    const toggleAudioBridge = vi.fn();

    const noPeer = buildCommands(makeCtx({ kvmHasPeers: false }));
    const openCmdNoPeer = noPeer.find((c) => c.id === "open-url-on-linked-pc");
    expect(openCmdNoPeer?.label).toBe("Open URL on Linked PC");
    expect(openCmdNoPeer?.disabled).toBe("No linked PC connected");

    const withPeer = buildCommands(
      makeCtx({ kvmHasPeers: true, openUrlOnLinkedPc, toggleAudioBridge })
    );
    const openCmd = withPeer.find((c) => c.id === "open-url-on-linked-pc");
    const audioCmd = withPeer.find((c) => c.id === "toggle-audio-bridge");

    expect(openCmd?.disabled).toBeUndefined();
    expect(openCmd?.hint).toBe("Opens in the default browser of the Linked PC");
    expect(openCmd?.prompt?.placeholder).toBe("https://…");
    expect(openCmd?.prompt?.hint).toBe("Opens in the default browser of the Linked PC");
    expect(audioCmd?.label).toBe("Toggle audio bridge");
    expect(audioCmd?.disabled).toBeUndefined();

    // Selecting plain command does not send on its own; only prompt.onSubmit sends
    openCmd?.run();
    expect(openUrlOnLinkedPc).not.toHaveBeenCalled();

    openCmd?.prompt?.onSubmit("youtube.com");
    audioCmd?.run();
    expect(openUrlOnLinkedPc).toHaveBeenCalledWith("https://youtube.com");
    expect(toggleAudioBridge).toHaveBeenCalledTimes(1);
  });

  it("normalizes http/https/mailto and bare domains while rejecting other schemes", () => {
    expect(normalizeOpenableUrl("http://youtube.com")).toBe("http://youtube.com");
    expect(normalizeOpenableUrl("  https://example.com/watch?v=1  ")).toBe(
      "https://example.com/watch?v=1",
    );
    expect(normalizeOpenableUrl("mailto:team@example.com")).toBe(
      "mailto:team@example.com",
    );
    expect(normalizeOpenableUrl("youtube.com")).toBe("https://youtube.com");
    expect(normalizeOpenableUrl("docs.rs/tokio/latest")).toBe(
      "https://docs.rs/tokio/latest",
    );

    // Reject non-allowlisted schemes and plain search queries
    expect(normalizeOpenableUrl("javascript:alert(1)")).toBeNull();
    expect(normalizeOpenableUrl("file:///C:/Windows/System32")).toBeNull();
    expect(normalizeOpenableUrl("ftp://files.example.com")).toBeNull();
    expect(normalizeOpenableUrl("data:text/plain,hello")).toBeNull();
    expect(normalizeOpenableUrl("http://")).toBeNull();
    expect(normalizeOpenableUrl("mailto:")).toBeNull();
    expect(normalizeOpenableUrl("pair new device")).toBeNull();
    expect(normalizeOpenableUrl("")).toBeNull();
  });

  it("ranks direct URL query as top 'Open <url> on Linked PC' result with phone link second", () => {
    const openUrlOnLinkedPc = vi.fn();
    const openLinkOnPhone = vi.fn();
    const cmds = buildCommands(
      makeCtx({ kvmHasPeers: true, openUrlOnLinkedPc, openLinkOnPhone }),
    );

    const results = filterAndRankCommands(cmds, "http://youtube.com");
    expect(results[0]?.id).toBe("open-url-on-linked-pc");
    expect(results[0]?.label).toBe("Open http://youtube.com on Linked PC");
    expect(results[0]?.prompt).toBeUndefined();
    expect(results[1]?.id).toBe("open-link-on-phone");

    results[0]?.run();
    expect(openUrlOnLinkedPc).toHaveBeenCalledWith("http://youtube.com");

    results[1]?.run();
    expect(openLinkOnPhone).toHaveBeenCalledWith("http://youtube.com");

    const bareResults = filterAndRankCommands(cmds, "youtube.com");
    expect(bareResults[0]?.label).toBe("Open https://youtube.com on Linked PC");
  });
});
