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
    color: string;
    border: string;
  }[] = [
    {
      id: "flex",
      title: "Desktop Flex",
      badge: "DeX Virtual Workspace",
      description: "Secondary independent virtual display on Android 10+ with custom DPI scaling & app multi-windowing.",
      features: ["Custom DPI density", "Multi-window apps", "PC taskbar integration"],
      icon: Monitor,
      color: "text-cyan-400",
      border: "border-cyan-500/80",
    },
    {
      id: "mirror",
      title: "Phone Mirror",
      badge: "1:1 Native Portrait",
      description: "Direct zero-lag portrait screen mirror. Full touch, hardware key injection, and seamless wheel swipe navigation.",
      features: ["Low latency feed", "Reels wheel swipe", "Direct touch & keys"],
      icon: Smartphone,
      color: "text-emerald-400",
      border: "border-emerald-500/80",
    },
    {
      id: "camera",
      title: "Camera Studio",
      badge: "HD Device Webcam",
      description: "Transform your Android flagship lenses into an ultra-sharp desktop camera feed for streaming & calls.",
      features: ["Front / rear camera", "Torch toggle ready", "Ultra-low latency"],
      icon: Video,
      color: "text-amber-400",
      border: "border-amber-500/80",
    },
    {
      id: "hifi",
      title: "High Fidelity",
      badge: "60 FPS · 20 Mbps",
      description: "Maximum bitrate AV1/H.265 pipeline for crisp typography, mobile gaming, and media fidelity.",
      features: ["AV1 / HEVC codec", "20 Mbps bitrate", "Fluid 60 FPS"],
      icon: Zap,
      color: "text-purple-400",
      border: "border-purple-500/80",
    },
  ];

  return (
    <div className="space-y-2.5">
      <div className="flex items-center justify-between">
        <span className="font-mono text-[11px] font-bold uppercase tracking-wider text-zinc-400">
          LAUNCH MATRIX · WORKSTATION PROFILE
        </span>
        <span className="text-[11px] text-zinc-500">
          Select target virtualization mode
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
              className={`group relative flex flex-col justify-between rounded-xl border p-3.5 text-left transition-all active:scale-[0.98] ${
                isActive
                  ? `${p.border} bg-[#161a26] shadow-[0_0_20px_rgba(56,189,248,0.15)] ring-1 ring-cyan-500/40`
                  : "border-[#1e2330] bg-[#12151d]/70 hover:border-zinc-700 hover:bg-[#151924]"
              }`}
            >
              {/* Header Icon + Active Checkmark */}
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-2.5">
                  <div
                    className={`flex h-8 w-8 items-center justify-center rounded-lg border border-[#1e2330] bg-[#0a0c10] ${p.color}`}
                  >
                    <Icon className="h-4 w-4" />
                  </div>
                  <div>
                    <h3 className="text-xs font-bold text-zinc-100">{p.title}</h3>
                    <span className="font-mono text-[10px] text-zinc-500">{p.badge}</span>
                  </div>
                </div>

                {isActive && (
                  <span className="flex h-4 w-4 items-center justify-center rounded-full bg-cyan-500/20 text-cyan-400 ring-1 ring-cyan-400">
                    <Check className="h-2.5 w-2.5" />
                  </span>
                )}
              </div>

              {/* Description */}
              <p className="mt-2.5 text-[11px] leading-relaxed text-zinc-400">
                {p.description}
              </p>

              {/* Feature Tags */}
              <div className="mt-3 flex flex-wrap gap-1 border-t border-[#1e2330]/80 pt-2">
                {p.features.map((feat) => (
                  <span
                    key={feat}
                    className="rounded bg-[#0d1017] px-1.5 py-0.5 text-[9px] font-mono text-zinc-400 border border-[#1e2330]"
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
