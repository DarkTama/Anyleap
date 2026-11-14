import { useEffect, useState } from "react";
import { Camera, Loader2, Video, Volume2, VolumeX, X, Zap, Compass } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { selectClass } from "@/lib/styles";
import { listDeviceCameras, startCameraMirror } from "@/lib/tauri";
import type { CameraDeviceOption, CameraSettings } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";
import { logActivity } from "@/stores/activityStore";
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
      const msg = String(e);
      setError(msg);
      logActivity({
        kind: "camera",
        title: `Failed to start camera on ${deviceName}`,
        detail: msg,
      });
    } finally {
      setStarting(false);
    }
  }

  return (
    <div className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4">
      <Card className="modal-panel w-full max-w-md border-ink-700 bg-ink-900 shadow-elevated">
        <CardHeader className="flex-row items-center justify-between pb-3">
          <div className="flex items-center gap-2">
            <Camera className="h-5 w-5 text-gold-400" />
            <CardTitle className="text-base font-semibold">Camera Studio</CardTitle>
          </div>
          <Button size="icon" variant="ghost" onClick={onClose} aria-label="Close">
            <X className="h-4 w-4" />
          </Button>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="text-xs text-text-muted">
            Stream camera video directly from{" "}
            <span className="font-medium text-text-strong">
              {deviceName ?? serial}
            </span>{" "}
            as a PC camera source.
          </div>

          {/* Virtual Webcam / Google Meet Guide Card */}
          <div className="rounded-lg border border-mist-300/30 bg-ink-800 p-3 text-xs space-y-1.5">
            <div className="flex items-center gap-1.5 font-semibold text-mist-300">
              <Video className="h-3.5 w-3.5" />
              <span>Using with Google Meet, Zoom, or Discord:</span>
            </div>
            <ol className="list-decimal list-inside space-y-1 text-[11px] text-text-muted leading-relaxed">
              <li>Open <strong className="text-text-strong">OBS Studio</strong>.</li>
              <li>Add source: <strong className="text-text-strong">Window Capture → AnyLeap Camera Studio</strong>.</li>
              <li>Click <strong className="text-status-success">Start Virtual Camera</strong> in OBS.</li>
              <li>In Google Meet or Zoom, choose <strong className="text-mist-300">OBS Virtual Camera</strong>.</li>
            </ol>
          </div>
          {error && (
            <div className="rounded-md border border-status-danger/40 bg-status-danger-bg p-2.5 text-xs text-status-danger">
              {error}
            </div>
          )}

          {/* Camera Lens Selection */}
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-text">
              Camera Lens
            </label>
            {loading ? (
              <div className="flex items-center gap-2 rounded border border-ink-700 bg-ink-950 p-2 text-xs text-text-muted">
                <Loader2 className="h-3.5 w-3.5 animate-spin text-gold-400" />
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
                  <option key={c.id} value={c.id} className="bg-ink-900 text-text-strong">
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
              <label className="text-xs font-medium text-text">
                Resolution
              </label>
              <select
                className={selectClass}
                value={resolution}
                onChange={(e) => setResolution(e.target.value)}
              >
                <option value="1280x720" className="bg-ink-900 text-text-strong">720p HD (1280×720) — Recommended (Smooth Wireless)</option>
                <option value="800x480" className="bg-ink-900 text-text-strong">480p WVGA (800×480) — Low Latency / Fast</option>
                <option value="640x480" className="bg-ink-900 text-text-strong">480p VGA (640×480) — Minimum Bandwidth</option>
                <option value="1920x1080" className="bg-ink-900 text-text-strong">1080p FHD (1920×1080) — High Definition</option>
                <option value="3840x2160" className="bg-ink-900 text-text-strong">4K UHD (3840×2160) — Wired USB</option>
                <option value="" className="bg-ink-900 text-text-strong">Full Sensor (Auto)</option>
              </select>
            </div>

            <div className="space-y-1.5">
              <label className="text-xs font-medium text-text">
                Frame Rate
              </label>
              <select
                className={selectClass}
                value={fps}
                onChange={(e) => setFps(Number(e.target.value))}
              >
                <option value={30} className="bg-ink-900 text-text-strong">30 FPS (Standard)</option>
                <option value={60} className="bg-ink-900 text-text-strong">60 FPS (High Speed)</option>
                <option value={0} className="bg-ink-900 text-text-strong">Device Default</option>
              </select>
            </div>
          </div>

          {/* Orientation & Wireless Quality */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <label className="text-xs font-medium text-text flex items-center gap-1.5">
                <Compass className="h-3 w-3 text-mist-300" />
                Stream Orientation
              </label>
              <select
                className={selectClass}
                value={orientation}
                onChange={(e) => setOrientation(e.target.value)}
              >
                <option value="270" className="bg-ink-900 text-text-strong">Portrait (Upright Phone)</option>
                <option value="0" className="bg-ink-900 text-text-strong">Landscape (Monitor Wide)</option>
                <option value="90" className="bg-ink-900 text-text-strong">Inverted Portrait</option>
                <option value="180" className="bg-ink-900 text-text-strong">Inverted Landscape</option>
                <option value="auto" className="bg-ink-900 text-text-strong">Device Native Auto</option>
              </select>
            </div>

            <div className="space-y-1.5">
              <label className="text-xs font-medium text-text">
                Wireless Stream Profile
              </label>
              <select
                className={selectClass}
                value={bitrateMode}
                onChange={(e) => setBitrateMode(e.target.value)}
              >
                <option value="smooth" className="bg-ink-900 text-text-strong">Smooth (4 Mbps · 50ms buffer) — Best for Wi-Fi</option>
                <option value="balanced" className="bg-ink-900 text-text-strong">Balanced (8 Mbps · 50ms buffer)</option>
                <option value="low" className="bg-ink-900 text-text-strong">Low Bandwidth (2 Mbps · 50ms buffer)</option>
                <option value="maximum" className="bg-ink-900 text-text-strong">Maximum Quality (16 Mbps · 0ms)</option>
              </select>
            </div>
          </div>
          {/* Toggles */}
          <div className="rounded-lg border border-ink-700 bg-ink-950 p-3 space-y-2.5">
            <label className="flex items-center justify-between cursor-pointer text-xs">
              <span className="flex items-center gap-2 text-text">
                <Zap className="h-3.5 w-3.5 text-status-warning" />
                Turn on Torch / Flash
              </span>
              <input
                type="checkbox"
                checked={torch}
                onChange={(e) => setTorch(e.target.checked)}
                className="h-4 w-4 accent-gold-400"
              />
            </label>

            <label className="flex items-center justify-between cursor-pointer text-xs">
              <span className="flex items-center gap-2 text-text">
                {micAudio ? (
                  <Volume2 className="h-3.5 w-3.5 text-mist-300" />
                ) : (
                  <VolumeX className="h-3.5 w-3.5 text-text-muted" />
                )}
                Capture Microphone Audio
              </span>
              <input
                type="checkbox"
                checked={micAudio}
                onChange={(e) => setMicAudio(e.target.checked)}
                className="h-4 w-4 accent-gold-400"
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
