import type { ShellTab } from "@/components/shell/AppHeader";
import type { StudioMode } from "@/components/deck/LaunchMatrix";

export interface PaletteCommand {
  id: string;
  label: string;
  group: string;
  keywords?: string[];
  run: () => void | Promise<void>;
  disabled?: string;
}

export interface CommandPaletteContext {
  selectedSerial: string | null;
  hasOnlineDevice: boolean;
  hasActiveSession: boolean;
  kvmRunning: boolean;
  switchTab: (tab: ShellTab) => void;
  startMode: (mode: StudioMode) => void;
  stopMirroring: () => void;
  takeScreenshot: () => void;
  openPair: () => void;
  openDoctor: () => void;
  openTimeline: () => void;
  checkForUpdates: () => void;
  toggleKvmServer: () => void;
}

export function buildCommands(ctx: CommandPaletteContext): PaletteCommand[] {
  const noDeviceReason = !ctx.selectedSerial
    ? "No device selected"
    : !ctx.hasOnlineDevice
      ? "Selected device is offline"
      : undefined;

  return [
    // Navigation
    {
      id: "tab-devices",
      label: "Switch to Devices tab",
      group: "Navigation",
      keywords: ["studio", "deck", "phone", "android"],
      run: () => ctx.switchTab("devices"),
    },
    {
      id: "tab-phone",
      label: "Switch to Phone tab",
      group: "Navigation",
      keywords: ["files", "photos", "notifications", "storage", "phone"],
      run: () => ctx.switchTab("phone"),
    },
    {
      id: "tab-computers",
      label: "Switch to Computers tab",
      group: "Navigation",
      keywords: ["kvm", "pc", "topology"],
      run: () => ctx.switchTab("computers"),
    },
    {
      id: "tab-settings",
      label: "Switch to Settings tab",
      group: "Navigation",
      keywords: ["preferences", "quality", "config"],
      run: () => ctx.switchTab("settings"),
    },

    // Device & Streaming Modes
    {
      id: "mode-flex",
      label: "Start Desktop Flex mode",
      group: "Stream",
      keywords: ["dex", "mirror", "launch", "virtual display"],
      disabled: noDeviceReason,
      run: () => ctx.startMode("flex"),
    },
    {
      id: "mode-mirror",
      label: "Start Phone Mirror mode",
      group: "Stream",
      keywords: ["portrait", "screen", "launch"],
      disabled: noDeviceReason,
      run: () => ctx.startMode("mirror"),
    },
    {
      id: "mode-camera",
      label: "Start Camera Studio mode",
      group: "Stream",
      keywords: ["webcam", "lens", "video", "launch"],
      disabled: noDeviceReason,
      run: () => ctx.startMode("camera"),
    },
    {
      id: "mode-hifi",
      label: "Start High Fidelity mode",
      group: "Stream",
      keywords: ["av1", "hevc", "60fps", "gaming", "launch"],
      disabled: noDeviceReason,
      run: () => ctx.startMode("hifi"),
    },
    {
      id: "stop-mirror",
      label: "Stop mirroring",
      group: "Stream",
      keywords: ["end", "session", "kill", "stream"],
      disabled: ctx.hasActiveSession ? undefined : "No active session",
      run: () => ctx.stopMirroring(),
    },
    {
      id: "take-screenshot",
      label: "Take screenshot",
      group: "Device",
      keywords: ["screencap", "capture", "png", "photo"],
      disabled: noDeviceReason,
      run: () => ctx.takeScreenshot(),
    },

    // Tools & Diagnostics
    {
      id: "pair-device",
      label: "Pair new device",
      group: "Device",
      keywords: ["wifi", "wireless", "qr", "adb"],
      run: () => ctx.openPair(),
    },
    {
      id: "connection-doctor",
      label: "Run Connection Doctor",
      group: "Diagnostics",
      keywords: ["troubleshoot", "diagnose", "adb", "scrcpy", "health"],
      run: () => ctx.openDoctor(),
    },
    {
      id: "open-timeline",
      label: "Open activity timeline",
      group: "Diagnostics",
      keywords: ["history", "log", "events"],
      run: () => ctx.openTimeline(),
    },
    {
      id: "check-updates",
      label: "Check for updates",
      group: "App",
      keywords: ["version", "release", "upgrade"],
      run: () => ctx.checkForUpdates(),
    },
    {
      id: "toggle-kvm",
      label: ctx.kvmRunning ? "Stop KVM server" : "Start KVM server",
      group: "Computers",
      keywords: ["virtual kvm", "mouse", "keyboard", "server"],
      run: () => ctx.toggleKvmServer(),
    },
  ];
}

/**
 * Case-insensitive substring match over `label` + `keywords`, ranked:
 * 1. label prefix match
 * 2. label substring match
 * 3. keywords substring match
 */
export function filterAndRankCommands(
  commands: PaletteCommand[],
  query: string,
): PaletteCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;

  const prefixMatches: PaletteCommand[] = [];
  const labelMatches: PaletteCommand[] = [];
  const keywordMatches: PaletteCommand[] = [];

  for (const cmd of commands) {
    const lowLabel = cmd.label.toLowerCase();
    if (lowLabel.startsWith(q)) {
      prefixMatches.push(cmd);
    } else if (lowLabel.includes(q)) {
      labelMatches.push(cmd);
    } else if (cmd.keywords?.some((k) => k.toLowerCase().includes(q))) {
      keywordMatches.push(cmd);
    }
  }

  return [...prefixMatches, ...labelMatches, ...keywordMatches];
}
