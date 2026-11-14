import { Monitor, Smartphone, Video, Zap, Check } from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import type { CoreSettings } from "@/lib/types";

export type StudioMode = "flex" | "mirror" | "camera" | "hifi";

interface LaunchMatrixProps {
  currentMode: StudioMode;
  onSelectMode: (mode: StudioMode) => void;
}

export function LaunchMatrix({ currentMode, onSelectMode }: LaunchMatrixProps) {
  const settings = useAppStore((s) => s.settings);
  const setSettings = useAppStore((s) => s.setSettings);

  const handleSelect = (mode: StudioMode) => {
    onSelectMode(mode);

    // Apply smart preset defaults according to profile
    let updated: Partial<CoreSettings> = {};
    if (mode === "flex") {
      updated = {
        flexDisplay: true,
        embedded: true,
        noWindowAspectRatioLock: true,
        renderFit: "unscaled",
      };
    } else if (mode === "mirror") {
      updated = {
        flexDisplay: false,
        embedded: true,
        noWindowAspectRatioLock: false,
        renderFit: "letterbox",
      };
    } else if (mode === "camera") {
      updated = {
        flexDisplay: false,
        embedded: true,
        noAudio: true,
        stayAwake: true,
      };
    } else if (mode === "hifi") {
      updated = {
        flexDisplay: settings.flexDisplay,
        embedded: true,
        maxFps: 60,
        videoBitRate: 20_000_000,
        videoCodec: "av1",
      };
    }

    setSettings({ ...settings, ...updated });
  };

  const PROFILES: {
    id: StudioMode;
    title: string;
    badge: string;
    description: string;
    features: string[];
    icon: typeof Monitor;
  }[] = [
    {
      id: "flex",
      title: "Desktop Flex",
      badge: "DeX Workspace",
      description: "Secondary independent virtual display with custom DPI scaling & app multi-windowing.",
      features: ["Custom DPI scaling", "Multi-window apps", "PC taskbar integration"],
      icon: Monitor,
    },
    {
      id: "mirror",
      title: "Phone Mirror",
      badge: "1:1 Native Portrait",
      description: "Direct zero-lag portrait screen mirror with full touch and wheel swipe navigation.",
      features: ["Low latency feed", "Reels wheel swipe", "Direct touch & keys"],
      icon: Smartphone,
    },
    {
      id: "camera",
      title: "Camera Studio",
      badge: "HD Device Webcam",
      description: "Transform your Android flagship lenses into a sharp desktop camera feed for streaming & calls.",
      features: ["Front / rear camera", "Torch toggle ready", "Ultra-low latency"],
      icon: Video,
    },
    {
      id: "hifi",
      title: "High Fidelity",
      badge: "60 FPS · 20 Mbps",
      description: "Maximum bitrate AV1/H.265 pipeline for crisp typography, mobile gaming, and media fidelity.",
      features: ["AV1 / HEVC codec", "20 Mbps bitrate", "Fluid 60 FPS"],
      icon: Zap,
    },
  ];

  return (
    <div className="space-y-2.5">
      <div className="flex items-center justify-between">
        <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
          Virtualization Profile
        </span>
        <span className="text-[11px] text-text-muted">
          Select target streaming mode
        </span>
      </div>

      <div className="grid grid-cols-2 gap-3">
        {PROFILES.map((p) => {
          const isActive = currentMode === p.id;
          const Icon = p.icon;

          return (
            <button
              key={p.id}
              type="button"
              onClick={() => handleSelect(p.id)}
              className={`btn-tactile group relative flex flex-col justify-between rounded-xl border p-3.5 text-left cursor-pointer ${
                isActive
                  ? "border-gold-400 bg-ink-800"
                  : "border-ink-700 bg-ink-900 text-text hover:border-text-muted hover:bg-ink-800/70"
              }`}
            >
              {/* Header Icon + Active Status */}
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-2.5">
                  <div
                    className={`flex h-8 w-8 items-center justify-center rounded-lg border ${
                      isActive
                        ? "border-gold-400/50 bg-ink-950 text-gold-400"
                        : "border-ink-700 bg-ink-800 text-text-muted group-hover:text-text-strong"
                    }`}
                  >
                    <Icon className="h-4 w-4" />
                  </div>
                  <div>
                    <h3 className={`text-xs font-semibold ${isActive ? "text-text-strong" : "text-text"}`}>
                      {p.title}
                    </h3>
                    <span className="font-mono text-[10px] text-text-muted">{p.badge}</span>
                  </div>
                </div>

                {isActive && (
                  <span className="flex h-4 w-4 items-center justify-center rounded-full bg-gold-400 text-ink-950 font-bold">
                    <Check className="h-2.5 w-2.5" />
                  </span>
                )}
              </div>

              {/* Description */}
              <p className="mt-2.5 text-[11px] leading-relaxed text-text">
                {p.description}
              </p>

              {/* Feature Tags */}
              <div className="mt-3 flex flex-wrap gap-1 border-t border-ink-700 pt-2">
                {p.features.map((feat) => (
                  <span
                    key={feat}
                    className="rounded bg-ink-950 px-1.5 py-0.5 text-[9px] font-mono text-text-muted border border-ink-700"
                  >
                    {feat}
                  </span>
                ))}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
