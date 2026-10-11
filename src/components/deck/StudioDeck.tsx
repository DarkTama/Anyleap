import { useState, useMemo } from "react";
import {
  Monitor,
  RefreshCw,
  Play,
  Square,
  Sparkles,
  CheckCircle2,
  EyeOff,
  Sun,
  MousePointer2,
} from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import {
  listDevices,
  startMirror,
  stopMirror,
  getSystemResolution,
  connectDevice,
  discoverWireless,
  resetDeviceDisplay,
} from "@/lib/tauri";
import { DeviceRack } from "./DeviceRack";
import { LaunchMatrix, type StudioMode } from "./LaunchMatrix";
import { DpiVisualizer } from "./DpiVisualizer";
import { CameraLaunchDialog } from "@/components/CameraLaunchDialog";
import type { DeviceInfo, SavedDevice } from "@/lib/types";

interface StudioDeckProps {
  onOpenPair?: () => void;
}

export function StudioDeck({ onOpenPair: _onOpenPair }: StudioDeckProps) {
  const devices = useAppStore((s) => s.devices);
  const savedDevices = useAppStore((s) => s.savedDevices);
  const sessions = useAppStore((s) => s.sessions);
  const setDevices = useAppStore((s) => s.setDevices);
  const settings = useAppStore((s) => s.settings);
  const setSettings = useAppStore((s) => s.setSettings);
  const setError = useAppStore((s) => s.setError);
  const nicknames = useAppStore((s) => s.nicknames);

  const [selectedSerial, setSelectedSerial] = useState<string | null>(null);
  const [currentMode, setCurrentMode] = useState<StudioMode>("flex");
  const [refreshing, setRefreshing] = useState(false);
  const [launching, setLaunching] = useState(false);
  const [cameraOpen, setCameraOpen] = useState(false);

  // Active target device resolution
  const activeDevice: DeviceInfo | SavedDevice | null = useMemo(() => {
    if (!selectedSerial) return devices[0] ?? savedDevices[0] ?? null;
    const foundDev = devices.find((d) => d.serial === selectedSerial);
    if (foundDev) return foundDev;
    const foundSaved = savedDevices.find(
      (s) => s.lastSerial === selectedSerial || `${s.host}:${s.port}` === selectedSerial,
    );
    return foundSaved ?? null;
  }, [devices, savedDevices, selectedSerial]);

  const activeSession = useMemo(() => {
    if (!selectedSerial) return null;
    return sessions.find((s) => s.serial === selectedSerial) ?? null;
  }, [sessions, selectedSerial]);

  const isLive = Boolean(
    activeDevice &&
      ("state" in activeDevice
        ? activeDevice.state === "device"
        : devices.some((d) => d.serial.startsWith(`${(activeDevice as SavedDevice).host}:`))),
  );

  // Parse current DPI from flexDisplaySize or fallback to 160
  const currentDpi = useMemo(() => {
    const raw = settings.flexDisplaySize || "";
    const m = raw.match(/\/(\d+)/);
    return m ? Number.parseInt(m[1], 10) : 160;
  }, [settings.flexDisplaySize]);

  const handleDpiChange = (newDpi: number) => {
    const raw = settings.flexDisplaySize || "";
    const slashIdx = raw.indexOf("/");
    const baseRes = slashIdx !== -1 ? raw.slice(0, slashIdx) : raw || "1920x1080";
    setSettings({
      ...settings,
      flexDisplaySize: `${baseRes}/${newDpi}`,
    });
  };

  const handleSetPCResolution = async () => {
    try {
      const [w, h] = await getSystemResolution();
      setSettings({
        ...settings,
        flexDisplaySize: `${w}x${h}/${currentDpi}`,
      });
    } catch (e) {
      setError(String(e));
    }
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    setError(null);
    try {
      setDevices(await listDevices());
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  };

  const handlePrimaryLaunch = async () => {
    if (!selectedSerial) return;
    if (currentMode === "camera") {
      setCameraOpen(true);
      return;
    }

    setLaunching(true);
    setError(null);

    try {
      let targetSerial = selectedSerial;
      const isSavedOffline =
        !devices.some((d) => d.serial === targetSerial) &&
        savedDevices.some((s) => `${s.host}:${s.port}` === targetSerial || s.lastSerial === targetSerial);

      if (isSavedOffline) {
        const saved = savedDevices.find(
          (s) => `${s.host}:${s.port}` === targetSerial || s.lastSerial === targetSerial,
        );
        if (saved) {
          const services = await discoverWireless().catch(() => []);
          const svc = services.find((s) => s.host === saved.host && s.serviceType.includes("connect"));
          const connectedSerial = await connectDevice(saved.host, svc?.port ?? saved.port);
          targetSerial = connectedSerial;
          setDevices(await listDevices());
        }
      }

      if (activeSession) {
        await stopMirror(activeSession.id);
      } else {
        await startMirror(targetSerial, settings);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLaunching(false);
    }
  };

  const targetDisplayName = activeDevice
    ? "model" in activeDevice
      ? activeDevice.model || activeDevice.product || activeDevice.serial
      : activeDevice.label || activeDevice.lastSerial || `${activeDevice.host}:${activeDevice.port}`
    : "No Device Selected";

  const targetNickname = selectedSerial ? nicknames[selectedSerial] : undefined;

  return (
    <div className="flex h-full w-full overflow-hidden select-none bg-[#0c0c0e]">
      {/* Left Column: Device Hardware Rack */}
      <DeviceRack
        selectedSerial={selectedSerial}
        onSelectDevice={(serial) => setSelectedSerial(serial)}
      />

      {/* Right Column: Workbench & Launch Bay */}
      <div className="flex flex-1 flex-col overflow-y-auto bg-[#0c0c0e] p-5 space-y-4">
        {/* Selected Target Device Banner */}
        <div className="flex items-center justify-between rounded-xl border border-[#222226] bg-[#141417] px-4 py-3">
          <div className="flex items-center gap-3">
            <div
              className={`flex h-9 w-9 items-center justify-center rounded-lg border ${
                isLive
                  ? "border-emerald-500/50 bg-emerald-500/10 text-emerald-400"
                  : "border-[#27272a] bg-[#18181c] text-zinc-400"
              }`}
            >
              <Monitor className="h-4.5 w-4.5" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <span className="font-semibold text-xs text-zinc-100">
                  {targetDisplayName}
                </span>
                {targetNickname && (
                  <span className="rounded bg-[#1a1a20] px-1.5 py-0.2 font-mono text-[9px] text-zinc-400 border border-[#27272a]">
                    {targetNickname}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2 font-mono text-[10px] text-zinc-400 mt-0.5">
                <span>{selectedSerial || "No Device Selected"}</span>
                <span>·</span>
                <span className={isLive ? "text-emerald-400" : "text-zinc-500"}>
                  {isLive ? (activeSession ? "STREAMING" : "CONNECTED") : "OFFLINE / SAVED"}
                </span>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={handleRefresh}
              disabled={refreshing}
              className="btn-tactile flex items-center gap-1.5 rounded-lg border border-[#27272a] bg-[#16161a] px-2.5 py-1 text-xs font-medium text-zinc-300 hover:border-zinc-600 hover:bg-[#1c1c21] cursor-pointer"
              title="Rescan devices"
            >
              <RefreshCw className={`h-3 w-3 ${refreshing ? "animate-spin text-zinc-200" : "text-zinc-400"}`} />
              <span className="hidden sm:inline">Refresh</span>
            </button>
          </div>
        </div>

        {/* Mode Matrix Grid */}
        <LaunchMatrix currentMode={currentMode} onSelectMode={setCurrentMode} />

        {/* Dynamic Mode-Specific Parameter Dock */}
        {currentMode === "flex" && (
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-zinc-400">
                Flex Desktop Tuning
              </span>
              <div className="flex items-center gap-3">
                <button
                  type="button"
                  onClick={() => {
                    if (selectedSerial) {
                      resetDeviceDisplay(selectedSerial).catch((e) => setError(String(e)));
                    }
                  }}
                  className="text-[10px] font-mono text-zinc-400 hover:text-amber-300 underline cursor-pointer"
                >
                  Reset Density
                </button>
                <button
                  type="button"
                  onClick={handleSetPCResolution}
                  className="text-[10px] font-mono text-zinc-300 hover:text-white underline cursor-pointer"
                >
                  Match PC Resolution
                </button>
              </div>
            </div>

            <DpiVisualizer dpi={currentDpi} onChange={handleDpiChange} />

            {/* Quick Toggles */}
            <div className="grid grid-cols-2 gap-3">
              <div className="flex items-center justify-between rounded-xl border border-[#222226] bg-[#141417] p-3">
                <div>
                  <div className="flex items-center gap-1.5">
                    <EyeOff className="h-3.5 w-3.5 text-zinc-400" />
                    <span className="text-xs font-semibold text-zinc-200">
                      Hide Android Taskbar
                    </span>
                  </div>
                  <p className="mt-0.5 text-[10px] text-zinc-500">
                    Removes bottom bar for full desktop area.
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() =>
                    setSettings({
                      ...settings,
                      hideVirtualTaskbar: !settings.hideVirtualTaskbar,
                    })
                  }
                  className={`relative inline-flex h-4.5 w-8 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-150 ease-out ${
                    settings.hideVirtualTaskbar ? "bg-zinc-100" : "bg-[#27272a]"
                  }`}
                >
                  <span
                    className={`pointer-events-none inline-block h-3.5 w-3.5 transform rounded-full shadow ring-0 transition duration-150 ease-out ${
                      settings.hideVirtualTaskbar ? "translate-x-3.5 bg-zinc-950" : "translate-x-0 bg-zinc-400"
                    }`}
                  />
                </button>
              </div>

              <div className="flex items-center justify-between rounded-xl border border-[#222226] bg-[#141417] p-3">
                <div>
                  <div className="flex items-center gap-1.5">
                    <Sun className="h-3.5 w-3.5 text-zinc-400" />
                    <span className="text-xs font-semibold text-zinc-200">
                      Turn Phone Screen Off
                    </span>
                  </div>
                  <p className="mt-0.5 text-[10px] text-zinc-500">
                    Saves battery and protects display.
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() =>
                    setSettings({
                      ...settings,
                      turnScreenOff: !settings.turnScreenOff,
                    })
                  }
                  className={`relative inline-flex h-4.5 w-8 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-150 ease-out ${
                    settings.turnScreenOff ? "bg-zinc-100" : "bg-[#27272a]"
                  }`}
                >
                  <span
                    className={`pointer-events-none inline-block h-3.5 w-3.5 transform rounded-full shadow ring-0 transition duration-150 ease-out ${
                      settings.turnScreenOff ? "translate-x-3.5 bg-zinc-950" : "translate-x-0 bg-zinc-400"
                    }`}
                  />
                </button>
              </div>
            </div>
          </div>
        )}

        {currentMode === "mirror" && (
          <div className="space-y-3">
            <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-zinc-400">
              Mirror Tuning
            </span>
            <div className="grid grid-cols-2 gap-3">
              <div className="flex items-center justify-between rounded-xl border border-[#222226] bg-[#141417] p-3">
                <div>
                  <div className="flex items-center gap-1.5">
                    <MousePointer2 className="h-3.5 w-3.5 text-zinc-400" />
                    <span className="text-xs font-semibold text-zinc-200">Show Touches</span>
                  </div>
                  <p className="mt-0.5 text-[10px] text-zinc-500">
                    Draw visual touch points on screen.
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() =>
                    setSettings({
                      ...settings,
                      showTouches: !settings.showTouches,
                    })
                  }
                  className={`relative inline-flex h-4.5 w-8 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-150 ease-out ${
                    settings.showTouches ? "bg-zinc-100" : "bg-[#27272a]"
                  }`}
                >
                  <span
                    className={`pointer-events-none inline-block h-3.5 w-3.5 transform rounded-full shadow ring-0 transition duration-150 ease-out ${
                      settings.showTouches ? "translate-x-3.5 bg-zinc-950" : "translate-x-0 bg-zinc-400"
                    }`}
                  />
                </button>
              </div>

              <div className="flex items-center justify-between rounded-xl border border-[#222226] bg-[#141417] p-3">
                <div>
                  <div className="flex items-center gap-1.5">
                    <Sun className="h-3.5 w-3.5 text-zinc-400" />
                    <span className="text-xs font-semibold text-zinc-200">
                      Physical Screen Off
                    </span>
                  </div>
                  <p className="mt-0.5 text-[10px] text-zinc-500">
                    Darken device during stream.
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() =>
                    setSettings({
                      ...settings,
                      turnScreenOff: !settings.turnScreenOff,
                    })
                  }
                  className={`relative inline-flex h-4.5 w-8 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-150 ease-out ${
                    settings.turnScreenOff ? "bg-zinc-100" : "bg-[#27272a]"
                  }`}
                >
                  <span
                    className={`pointer-events-none inline-block h-3.5 w-3.5 transform rounded-full shadow ring-0 transition duration-150 ease-out ${
                      settings.turnScreenOff ? "translate-x-3.5 bg-zinc-950" : "translate-x-0 bg-zinc-400"
                    }`}
                  />
                </button>
              </div>
            </div>
          </div>
        )}

        {currentMode === "camera" && (
          <div className="rounded-xl border border-[#222226] bg-[#141417] p-4 space-y-2">
            <div className="flex items-center gap-2">
              <CheckCircle2 className="h-4 w-4 text-emerald-400" />
              <span className="text-xs font-semibold text-zinc-200">
                Camera Studio Ready
              </span>
            </div>
            <p className="text-xs text-zinc-400 leading-relaxed">
              Streams raw Android camera sensor feed directly to your PC with custom resolution and framerate controls.
            </p>
          </div>
        )}

        {currentMode === "hifi" && (
          <div className="rounded-xl border border-[#222226] bg-[#141417] p-4">
            <div className="flex items-center gap-2">
              <Sparkles className="h-4 w-4 text-zinc-300" />
              <span className="text-xs font-semibold text-zinc-200">
                AV1 / HEVC High-Bitrate Pipeline (20 Mbps · 60 FPS)
              </span>
            </div>
            <p className="mt-1 text-xs text-zinc-400 leading-relaxed">
              Calibrated for crystal sharp graphics, mobile gaming and typography. Best over 5 GHz Wi-Fi or USB.
            </p>
          </div>
        )}

        {/* Primary Launch Action Bar */}
        <div className="rounded-xl border border-[#222226] bg-[#141417] p-3.5 space-y-3">
          <div className="flex items-center justify-between text-[11px] font-mono text-zinc-400">
            <div className="flex items-center gap-2">
              <span className="text-zinc-500">TARGET:</span>
              <span className="text-zinc-200 font-semibold">{selectedSerial || "None"}</span>
              <span>·</span>
              <span className="text-zinc-300">
                {currentMode === "flex" ? `FLEX (${currentDpi} DPI)` : currentMode.toUpperCase()}
              </span>
            </div>
            <div className="text-zinc-500">CONTAINER: EMBEDDED WINDOW</div>
          </div>

          {activeSession ? (
            <button
              type="button"
              disabled={launching}
              onClick={handlePrimaryLaunch}
              className="btn-tactile w-full flex items-center justify-center gap-2 rounded-lg bg-rose-600 py-3 text-xs font-semibold text-white hover:bg-rose-500 cursor-pointer shadow-sm"
            >
              <Square className="h-3.5 w-3.5 fill-white" />
              <span>Stop Active Stream</span>
            </button>
          ) : (
            <button
              type="button"
              disabled={launching || !selectedSerial}
              onClick={handlePrimaryLaunch}
              className="btn-tactile w-full flex items-center justify-center gap-2 rounded-lg bg-white py-3 text-xs font-semibold text-zinc-950 hover:bg-zinc-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed shadow-sm"
            >
              <Play className="h-3.5 w-3.5 fill-zinc-950" />
              <span>
                {currentMode === "camera"
                  ? "Configure & Launch Camera Studio"
                  : `Launch ${currentMode === "flex" ? "Desktop Flex" : currentMode === "mirror" ? "Phone Mirror" : "High Fidelity"}`}
              </span>
            </button>
          )}
        </div>
      </div>

      {/* Camera Launch Dialog */}
      {selectedSerial && (
        <CameraLaunchDialog
          isOpen={cameraOpen}
          onClose={() => setCameraOpen(false)}
          serial={selectedSerial}
          deviceName={targetDisplayName}
        />
      )}
    </div>
  );
}
