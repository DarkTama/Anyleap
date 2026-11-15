import type { ShellTab } from "@/components/shell/AppHeader";
import type { StudioMode } from "@/components/deck/LaunchMatrix";

export interface PaletteCommandPrompt {
  placeholder: string;
  hint: string;
  getInitialValue?: () => Promise<string> | string;
  onSubmit: (value: string) => void | Promise<void>;
}

export interface PaletteCommand {
  id: string;
  label: string;
  group: string;
  hint?: string;
  keywords?: string[];
  prompt?: PaletteCommandPrompt;
  run: () => void | Promise<void>;
  runWithUrl?: (url: string) => void | Promise<void>;
  disabled?: string;
}

/**
 * Detects and normalizes an openable URL from user input.
 * Allow-list: http://, https://, and mailto: only.
 * Bare domains (e.g. "youtube.com" or "docs.rs/tokio") are prefixed with "https://".
 */
export function normalizeOpenableUrl(query: string): string | null {
  const trimmed = query.trim();
  if (!trimmed || /\s/.test(trimmed)) return null;

  const schemeMatch = trimmed.match(/^([a-zA-Z][a-zA-Z0-9+.-]*):(.*)$/);
  if (schemeMatch) {
    const scheme = schemeMatch[1].toLowerCase();
    const rest = schemeMatch[2];
    if (scheme === "mailto") {
      const addr = rest.trim();
      if (!addr || addr.startsWith("//") || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(addr.split("?")[0])) {
        return null;
      }
      return `mailto:${addr}`;
    }
    if (scheme !== "http" && scheme !== "https") {
      return null;
    }
    if (!rest.startsWith("//")) return null;
    const normalized = `${scheme}:${rest}`;
    try {
      const parsed = new URL(normalized);
      if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
      if (!parsed.hostname || !parsed.hostname.includes(".")) return null;
      return normalized;
    } catch {
      return null;
    }
  }

  // Bare domain: must start with host.tld (TLD >= 2 alpha chars), optional :port and /path
  const bareDomainPattern =
    /^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?)*\.[a-zA-Z]{2,}(?::\d{1,5})?(?:[/?#].*)?$/;
  if (!bareDomainPattern.test(trimmed)) return null;

  const withHttps = `https://${trimmed}`;
  try {
    const parsed = new URL(withHttps);
    if (!parsed.hostname) return null;
    return withHttps;
  } catch {
    return null;
  }
}

export interface CommandPaletteContext {
  selectedSerial: string | null;
  hasOnlineDevice: boolean;
  hasActiveSession: boolean;
  isRecording: boolean;
  kvmRunning: boolean;
  kvmHasPeers: boolean;
  switchTab: (tab: ShellTab) => void;
  startMode: (mode: StudioMode) => void;
  stopMirroring: () => void;
  toggleRecording: () => void;
  openLinkOnPhone: (url?: string) => void | Promise<void>;
  togglePhoneWifi: () => void;
  togglePhoneBluetooth: () => void;
  togglePhoneDnd: () => void;
  takeScreenshot: () => void;
  openPair: () => void;
  openDoctor: () => void;
  openTimeline: () => void;
  checkForUpdates: () => void;
  toggleKvmServer: () => void;
  openUrlOnLinkedPc: (url?: string) => void | Promise<void>;
  toggleAudioBridge: () => void;
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
      id: "toggle-recording",
      label: ctx.isRecording ? "Stop recording" : "Start recording",
      group: "Stream",
      keywords: ["record", "screen", "video", "mp4"],
      disabled: ctx.isRecording
        ? undefined
        : (noDeviceReason ?? (ctx.hasActiveSession ? undefined : "No active session")),
      run: () => ctx.toggleRecording(),
    },
    {
      id: "open-link-on-phone",
      label: "Open link on phone",
      group: "Device",
      hint: "Opens in the default browser on the connected phone.",
      keywords: ["url", "browser", "clipboard", "link"],
      disabled: noDeviceReason,
      prompt: {
        placeholder: "https://…",
        hint: "Opens in the default browser on the connected phone.",
        getInitialValue: async () => {
          try {
            const clip = (await navigator.clipboard.readText()).trim();
            return normalizeOpenableUrl(clip) ?? "";
          } catch {
            // clipboard unavailable
          }
          return "";
        },
        onSubmit: (rawUrl) => {
          const normalized = normalizeOpenableUrl(rawUrl);
          if (!normalized) return;
          return ctx.openLinkOnPhone(normalized);
        },
      },
      run: () => ctx.openLinkOnPhone(),
      runWithUrl: (url) => ctx.openLinkOnPhone(url),
    },
    {
      id: "toggle-phone-wifi",
      label: "Toggle phone Wi-Fi",
      group: "Device",
      keywords: ["wifi", "wireless", "network"],
      disabled: noDeviceReason,
      run: () => ctx.togglePhoneWifi(),
    },
    {
      id: "toggle-phone-bluetooth",
      label: "Toggle phone Bluetooth",
      group: "Device",
      keywords: ["bluetooth", "bt"],
      disabled: noDeviceReason,
      run: () => ctx.togglePhoneBluetooth(),
    },
    {
      id: "toggle-phone-dnd",
      label: "Toggle phone Do Not Disturb",
      group: "Device",
      keywords: ["dnd", "mute", "silent", "notifications"],
      disabled: noDeviceReason,
      run: () => ctx.togglePhoneDnd(),
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
    {
      id: "open-url-on-linked-pc",
      label: "Open URL on Linked PC",
      group: "Computers",
      hint: "Opens in the default browser of the Linked PC",
      keywords: ["url", "link", "browser", "continuity", "open"],
      disabled: ctx.kvmHasPeers ? undefined : "No linked PC connected",
      prompt: {
        placeholder: "https://…",
        hint: "Opens in the default browser of the Linked PC",
        getInitialValue: async () => {
          try {
            const clip = (await navigator.clipboard.readText()).trim();
            return normalizeOpenableUrl(clip) ?? "";
          } catch {
            // clipboard unavailable
          }
          return "";
        },
        onSubmit: (rawUrl) => {
          const normalized = normalizeOpenableUrl(rawUrl);
          if (!normalized) return;
          return ctx.openUrlOnLinkedPc(normalized);
        },
      },
      run: () => {},
      runWithUrl: (url) => ctx.openUrlOnLinkedPc(url),
    },
    {
      id: "toggle-audio-bridge",
      label: "Toggle audio bridge",
      group: "Computers",
      keywords: ["audio", "sound", "bridge", "wasapi", "continuity"],
      run: () => ctx.toggleAudioBridge(),
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
  const trimmed = query.trim();
  if (!trimmed) return commands;

  const normalizedUrl = normalizeOpenableUrl(trimmed);
  const urlMatches: PaletteCommand[] = [];

  if (normalizedUrl) {
    const linkedCmd = commands.find((c) => c.id === "open-url-on-linked-pc");
    if (linkedCmd && !linkedCmd.disabled) {
      urlMatches.push({
        ...linkedCmd,
        label: `Open ${normalizedUrl} on Linked PC`,
        prompt: undefined,
        run: () =>
          linkedCmd.runWithUrl
            ? linkedCmd.runWithUrl(normalizedUrl)
            : linkedCmd.prompt
              ? linkedCmd.prompt.onSubmit(normalizedUrl)
              : linkedCmd.run(),
      });
    }
    const phoneCmd = commands.find((c) => c.id === "open-link-on-phone");
    if (phoneCmd && !phoneCmd.disabled) {
      urlMatches.push({
        ...phoneCmd,
        label: `Open ${normalizedUrl} on phone`,
        prompt: undefined,
        run: () =>
          phoneCmd.runWithUrl
            ? phoneCmd.runWithUrl(normalizedUrl)
            : phoneCmd.prompt
              ? phoneCmd.prompt.onSubmit(normalizedUrl)
              : phoneCmd.run(),
      });
    }
  }

  const q = trimmed.toLowerCase();
  const urlMatchIds = new Set(urlMatches.map((c) => c.id));
  const prefixMatches: PaletteCommand[] = [];
  const labelMatches: PaletteCommand[] = [];
  const keywordMatches: PaletteCommand[] = [];

  for (const cmd of commands) {
    if (urlMatchIds.has(cmd.id)) continue;
    const lowLabel = cmd.label.toLowerCase();
    if (lowLabel.startsWith(q)) {
      prefixMatches.push(cmd);
    } else if (lowLabel.includes(q)) {
      labelMatches.push(cmd);
    } else if (cmd.keywords?.some((k) => k.toLowerCase().includes(q))) {
      keywordMatches.push(cmd);
    }
  }

  return [...urlMatches, ...prefixMatches, ...labelMatches, ...keywordMatches];
}
