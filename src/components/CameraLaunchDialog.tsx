import { useEffect, useState } from "react";
import { Camera, Loader2, Video, Volume2, VolumeX, X, Zap, Compass } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { selectClass } from "@/lib/styles";
import { listDeviceCameras, startCameraMirror } from "@/lib/tauri";
import type { CameraDeviceOption, CameraSettings } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";
interface CameraLaunchDialogProps {
  isOpen: boolean;
  onClose: () => void;
  serial: string;
  deviceName: string | null;
  onStarted?: () => void;
}

export function CameraLaunchDialog({
  isOpen,
  onClose,
  serial,
  deviceName,
  onStarted,
}: CameraLaunchDialogProps) {
  const [loading, setLoading] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [cameras, setCameras] = useState<CameraDeviceOption[]>([]);
  const [facing, setFacing] = useState<"back" | "front" | "external">("back");
  const [selectedId, setSelectedId] = useState<string>("");
  const [resolution, setResolution] = useState<string>("1280x720");
  const [fps, setFps] = useState<number>(30);
  const [orientation, setOrientation] = useState<string>("270");
  const [bitrateMode, setBitrateMode] = useState<string>("smooth");
  const [torch, setTorch] = useState<boolean>(false);
  const [micAudio, setMicAudio] = useState<boolean>(true);
  useEffect(() => {
    if (!isOpen) return;

    let active = true;
    setLoading(true);
    setError(null);

    listDeviceCameras(serial)
      .then((cams) => {
        if (!active) return;
        if (cams && cams.length > 0) {
          setCameras(cams);
          const first = cams.find((c) => c.facing === "back") || cams[0];
          setSelectedId(first.id);
          setFacing(first.facing === "front" ? "front" : "back");
        } else {
          setCameras([]);
        }
      })
      .catch((_e) => {
        if (!active) return;
        // Fallback: device or scrcpy couldn't list cameras (e.g. < Android 12 or adb permissions)
        setCameras([]);
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [isOpen, serial]);

  if (!isOpen) return null;

  async function handleLaunch() {
    setStarting(true);
    setError(null);
    try {
      const isEmbedded = useAppStore.getState().settings.embedded;
      let videoBitRate: number | undefined = 4_000_000;
      if (bitrateMode === "low") videoBitRate = 2_000_000;
      if (bitrateMode === "balanced") videoBitRate = 8_000_000;
      if (bitrateMode === "maximum") videoBitRate = 16_000_000;

      const settings: CameraSettings = {
        facing,
        cameraId: selectedId || undefined,
        size: resolution || undefined,
        fps: fps > 0 ? fps : undefined,
        orientation: orientation !== "auto" ? orientation : undefined,
        videoBitRate,
        videoBuffer: 50,
        highSpeed: false,
        torch,
        noAudio: !micAudio,
        embedded: isEmbedded,
      };
      await startCameraMirror(serial, settings);
      onClose();
      onStarted?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
      <Card className="w-full max-w-md border-zinc-200 bg-white shadow-2xl dark:border-zinc-800 dark:bg-zinc-950">
        <CardHeader className="flex-row items-center justify-between pb-3">
          <div className="flex items-center gap-2">
            <Camera className="h-5 w-5 text-emerald-500" />
            <CardTitle className="text-base font-semibold">Camera Studio</CardTitle>
          </div>
          <Button size="icon" variant="ghost" onClick={onClose} aria-label="Close">
            <X className="h-4 w-4" />
          </Button>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="text-xs text-zinc-500">
            Stream high-quality camera video directly from{" "}
            <span className="font-medium text-zinc-900 dark:text-zinc-100">
              {deviceName ?? serial}
            </span>{" "}
            as a PC camera source.
          </div>

          {error && (
            <div className="rounded-md border border-rose-200 bg-rose-50 p-2.5 text-xs text-rose-700 dark:border-rose-900/50 dark:bg-rose-950/40 dark:text-rose-300">
              {error}
            </div>
          )}

          {/* Camera Lens Selection */}
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-zinc-700 dark:text-zinc-300">
              Camera Lens
            </label>
            {loading ? (
              <div className="flex items-center gap-2 rounded border border-zinc-200 p-2 text-xs text-zinc-500 dark:border-zinc-800">
                <Loader2 className="h-3.5 w-3.5 animate-spin text-emerald-500" />
                Querying cameras on device...
              </div>
            ) : cameras.length > 0 ? (
              <select
                className={selectClass}
                value={selectedId}
                onChange={(e) => {
                  const id = e.target.value;
                  setSelectedId(id);
                  const matched = cameras.find((c) => c.id === id);
                  if (matched) {
                    setFacing(matched.facing === "front" ? "front" : "back");
                  }
                }}
              >
                {cameras.map((c) => (
                  <option key={c.id} value={c.id}>
                    Camera {c.id} ({c.facing.toUpperCase()}) — {c.resolution}
                  </option>
                ))}
              </select>
            ) : (
              <div className="grid grid-cols-2 gap-2">
                <Button
                  size="sm"
                  type="button"
                  variant={facing === "back" ? "default" : "outline"}
                  onClick={() => {
                    setFacing("back");
                    setSelectedId("");
                  }}
                >
                  Rear / Back Camera
                </Button>
                <Button
                  size="sm"
                  type="button"
                  variant={facing === "front" ? "default" : "outline"}
                  onClick={() => {
                    setFacing("front");
                    setSelectedId("");
                  }}
                >
                  Front / Selfie Camera
                </Button>
              </div>
            )}
          </div>

          {/* Resolution & Frame Rate */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-zinc-700 dark:text-zinc-300">
                Resolution
              </label>
              <select
                className={selectClass}
                value={resolution}
                onChange={(e) => setResolution(e.target.value)}
              >
                <option value="1280x720">720p HD (1280×720) — Recommended (Smooth Wireless)</option>
                <option value="800x480">480p WVGA (800×480) — Low Latency / Fast</option>
                <option value="640x480">480p VGA (640×480) — Minimum Bandwidth</option>
                <option value="1920x1080">1080p FHD (1920×1080) — High Definition</option>
                <option value="3840x2160">4K UHD (3840×2160) — Wired USB</option>
                <option value="">Full Sensor (Auto)</option>
              </select>
            </div>

            <div className="space-y-1.5">
              <label className="text-xs font-medium text-zinc-700 dark:text-zinc-300">
                Frame Rate
              </label>
              <select
                className={selectClass}
                value={fps}
                onChange={(e) => setFps(Number(e.target.value))}
              >
                <option value={30}>30 FPS (Standard)</option>
                <option value={60}>60 FPS (High Speed)</option>
                <option value={0}>Device Default</option>
              </select>
            </div>
          </div>

          {/* Orientation & Wireless Quality */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-zinc-700 dark:text-zinc-300 flex items-center gap-1.5">
                <Compass className="h-3 w-3 text-cyan-400" />
                Stream Orientation
              </label>
              <select
                className={selectClass}
                value={orientation}
                onChange={(e) => setOrientation(e.target.value)}
              >
                <option value="270">Portrait (Upright Phone)</option>
                <option value="0">Landscape (Monitor Wide)</option>
                <option value="90">Inverted Portrait</option>
                <option value="180">Inverted Landscape</option>
                <option value="auto">Device Native Auto</option>
              </select>
            </div>

            <div className="space-y-1.5">
              <label className="text-xs font-medium text-zinc-700 dark:text-zinc-300">
                Wireless Stream Profile
              </label>
              <select
                className={selectClass}
                value={bitrateMode}
                onChange={(e) => setBitrateMode(e.target.value)}
              >
                <option value="smooth">Smooth (4 Mbps · 50ms buffer) — Best for Wi-Fi</option>
                <option value="balanced">Balanced (8 Mbps · 50ms buffer)</option>
                <option value="low">Low Bandwidth (2 Mbps · 50ms buffer)</option>
                <option value="maximum">Maximum Quality (16 Mbps · 0ms)</option>
              </select>
            </div>
          </div>
          {/* Toggles */}
          <div className="rounded-lg border border-zinc-200/80 p-3 space-y-2.5 dark:border-zinc-800">
            <label className="flex items-center justify-between cursor-pointer text-xs">
              <span className="flex items-center gap-2 text-zinc-700 dark:text-zinc-300">
                <Zap className="h-3.5 w-3.5 text-amber-500" />
                Turn on Torch / Flash
              </span>
              <input
                type="checkbox"
                checked={torch}
                onChange={(e) => setTorch(e.target.checked)}
                className="h-4 w-4 rounded border-zinc-300 text-emerald-600 focus:ring-emerald-500"
              />
            </label>

            <label className="flex items-center justify-between cursor-pointer text-xs">
              <span className="flex items-center gap-2 text-zinc-700 dark:text-zinc-300">
                {micAudio ? (
                  <Volume2 className="h-3.5 w-3.5 text-sky-500" />
                ) : (
                  <VolumeX className="h-3.5 w-3.5 text-zinc-400" />
                )}
                Capture Microphone Audio
              </span>
              <input
                type="checkbox"
                checked={micAudio}
                onChange={(e) => setMicAudio(e.target.checked)}
                className="h-4 w-4 rounded border-zinc-300 text-emerald-600 focus:ring-emerald-500"
              />
            </label>
          </div>

          {/* Action Buttons */}
          <div className="flex justify-end gap-2 pt-1">
            <Button variant="outline" size="sm" onClick={onClose} disabled={starting}>
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={handleLaunch}
              disabled={starting}
              className="bg-emerald-600 text-white hover:bg-emerald-700 dark:bg-emerald-500 dark:hover:bg-emerald-600"
            >
              {starting ? (
                <>
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  Starting...
                </>
              ) : (
                <>
                  <Video className="h-3.5 w-3.5" />
                  Start Camera
                </>
              )}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
