import { useState, useMemo } from "react";
import {
  Monitor,
  Settings as SettingsIcon,
  Wifi,
  RefreshCw,
  Play,
  Square,
  Sparkles,
  Sliders,
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
} from "@/lib/tauri";
import { DeviceRack } from "./DeviceRack";
import { LaunchMatrix, type StudioMode } from "./LaunchMatrix";
import { DpiVisualizer } from "./DpiVisualizer";
import { PairDialog } from "@/components/PairDialog";
import { CameraLaunchDialog } from "@/components/CameraLaunchDialog";
import { SettingsPanel } from "@/components/SettingsPanel";
import type { DeviceInfo, SavedDevice } from "@/lib/types";

export function StudioDeck() {
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
  const [pairOpen, setPairOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
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
    setLaunching(true);
    setError(null);
    try {
      if (activeSession) {
        await stopMirror(activeSession.id);
      } else if (currentMode === "camera") {
        setCameraOpen(true);
        return;
      } else {
        // If device is saved wireless and offline, try connecting first
        const isDeviceOffline = !devices.some((d) => d.serial === selectedSerial);
        let serialToLaunch = selectedSerial;

        if (isDeviceOffline) {
          const saved = savedDevices.find(
            (s) => s.lastSerial === selectedSerial || `${s.host}:${s.port}` === selectedSerial,
          );
          if (saved) {
            const services = await discoverWireless().catch(() => []);
            const svc = services.find(
              (s) => s.host === saved.host && s.serviceType.includes("connect"),
            );
            await connectDevice(saved.host, svc?.port ?? saved.port);
            const updatedDevices = await listDevices();
            setDevices(updatedDevices);
            const liveMatch = updatedDevices.find((d) => d.serial.startsWith(`${saved.host}:`));
            if (liveMatch) serialToLaunch = liveMatch.serial;
          }
        }

        await startMirror(serialToLaunch, settings);
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
      : (activeDevice as SavedDevice).label || (activeDevice as SavedDevice).lastSerial
    : "No Device Selected";

  const targetNickname = selectedSerial ? nicknames[selectedSerial] : undefined;

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-[#0a0c10] text-zinc-100 select-none">
      {/* Precision Instrument Deck Header */}
      <header className="flex h-13 shrink-0 items-center justify-between border-b border-[#1e2330] bg-[#0e1118] px-4">
        {/* Brand & System Status */}
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <div className="flex h-7 w-7 items-center justify-center rounded-lg border border-cyan-500/40 bg-cyan-500/10 text-cyan-400 shadow-[0_0_10px_rgba(56,189,248,0.2)]">
              <Sparkles className="h-4 w-4" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <span className="font-mono text-xs font-black tracking-widest text-zinc-100">
                  ANYLEAP
                </span>
                <span className="rounded border border-cyan-500/30 bg-cyan-500/10 px-1.5 py-0.2 font-mono text-[9px] font-bold text-cyan-400">
                  STUDIO DECK
                </span>
              </div>
              <p className="text-[10px] font-mono text-zinc-500">v0.7.0 HARDWARE WORKSTATION</p>
            </div>
          </div>

          <div className="h-4 w-px bg-[#1e2330]" />

          {/* Global System Telemetry */}
          <div className="flex items-center gap-2 font-mono text-[10px]">
            <div
              className={`h-2 w-2 rounded-full ${
                sessions.length > 0
                  ? "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.8)] animate-pulse"
                  : "bg-cyan-500/80"
              }`}
            />
            <span className="text-zinc-400">
              {sessions.length > 0 ? `${sessions.length} ACTIVE STREAM` : "READY"}
            </span>
          </div>
        </div>

        {/* Global Deck Actions */}
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleRefresh}
            disabled={refreshing}
            className="flex items-center gap-1.5 rounded-lg border border-[#1e2330] bg-[#141722] px-2.5 py-1.5 text-xs font-medium text-zinc-300 hover:border-zinc-700 hover:bg-[#181c2b] active:scale-[0.98] transition-all"
            title="Scan for USB & network devices"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${refreshing ? "animate-spin text-cyan-400" : ""}`} />
            <span className="hidden sm:inline">Refresh</span>
          </button>

          <button
            type="button"
            onClick={() => setPairOpen(true)}
            className="flex items-center gap-1.5 rounded-lg border border-cyan-500/40 bg-cyan-500/10 px-3 py-1.5 text-xs font-medium text-cyan-300 hover:bg-cyan-500/20 active:scale-[0.98] transition-all"
          >
            <Wifi className="h-3.5 w-3.5 text-cyan-400" />
            <span>Wireless Pair</span>
          </button>

          <button
            type="button"
            onClick={() => setSettingsOpen((v) => !v)}
            className={`flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium transition-all active:scale-[0.98] ${
              settingsOpen
                ? "border-cyan-500/80 bg-cyan-500/20 text-cyan-300 shadow-[0_0_10px_rgba(56,189,248,0.2)]"
                : "border-[#1e2330] bg-[#141722] text-zinc-300 hover:border-zinc-700 hover:bg-[#181c2b]"
            }`}
          >
            <Sliders className="h-3.5 w-3.5" />
            <span>Settings</span>
          </button>
        </div>
      </header>

      {/* Main Console Workstation */}
      <div className="flex flex-1 overflow-hidden">
        {/* Left Column: Device Hardware Rack */}
        <DeviceRack
          selectedSerial={selectedSerial}
          onSelectDevice={(serial) => setSelectedSerial(serial)}
        />

        {/* Right Column: Workbench & Launch Bay */}
        <div className="flex flex-1 flex-col overflow-y-auto bg-[#0a0c10] p-5 space-y-4.5">
          {settingsOpen ? (
            <div className="rounded-xl border border-[#1e2330] bg-[#12151d] p-4">
              <div className="flex items-center justify-between border-b border-[#1e2330] pb-3 mb-4">
                <div className="flex items-center gap-2">
                  <SettingsIcon className="h-4 w-4 text-cyan-400" />
                  <span className="font-mono text-xs font-bold uppercase tracking-wider text-zinc-200">
                    SYSTEM CONFIGURATION
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => setSettingsOpen(false)}
                  className="rounded bg-zinc-800 px-2 py-1 text-xs text-zinc-300 hover:bg-zinc-700 hover:text-white"
                >
                  Close Settings
                </button>
              </div>
              <SettingsPanel />
            </div>
          ) : (
            <>
              {/* Selected Target Device Banner */}
              <div className="flex items-center justify-between rounded-xl border border-[#1e2330] bg-[#12151d]/80 px-4 py-3 shadow-inner">
                <div className="flex items-center gap-3">
                  <div
                    className={`flex h-10 w-10 items-center justify-center rounded-xl border ${
                      isLive
                        ? "border-emerald-500/50 bg-emerald-500/10 text-emerald-400"
                        : "border-zinc-700 bg-zinc-800/50 text-zinc-400"
                    }`}
                  >
                    <Monitor className="h-5 w-5" />
                  </div>
                  <div>
                    <div className="flex items-center gap-2">
                      <span className="font-bold text-sm text-zinc-100">
                        {targetDisplayName}
                      </span>
                      {targetNickname && (
                        <span className="rounded bg-[#1e2330] px-1.5 py-0.2 font-mono text-[10px] text-zinc-400">
                          {targetNickname}
                        </span>
                      )}
                    </div>
                    <div className="flex items-center gap-2 font-mono text-[11px] text-zinc-400 mt-0.5">
                      <span>{selectedSerial || "No Device"}</span>
                      <span>·</span>
                      <span className={isLive ? "text-emerald-400" : "text-zinc-500"}>
                        {isLive ? (activeSession ? "STREAMING" : "CONNECTED") : "OFFLINE / SAVED"}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="flex items-center gap-2 font-mono text-[10px] text-zinc-400">
                  <span className="rounded bg-[#0d1017] px-2 py-1 border border-[#1e2330]">
                    EMBEDDED CONTAINER: ON
                  </span>
                  <span className="rounded bg-[#0d1017] px-2 py-1 border border-[#1e2330]">
                    UHID INPUT: ON
                  </span>
                </div>
              </div>

              {/* Mode Matrix Grid */}
              <LaunchMatrix currentMode={currentMode} onSelectMode={setCurrentMode} />

              {/* Mode Specific Bay */}
              {currentMode === "flex" && (
                <div className="space-y-3">
                  <div className="flex items-center justify-between">
                    <span className="font-mono text-[11px] font-bold uppercase tracking-wider text-zinc-400">
                      DESKTOP FLEX CONFIGURATION
                    </span>
                    <button
                      type="button"
                      onClick={handleSetPCResolution}
                      className="flex items-center gap-1 text-[11px] font-mono text-cyan-400 hover:text-cyan-300 underline underline-offset-2"
                    >
                      Match PC Display Resolution
                    </button>
                  </div>

                  {/* DPI Visualizer */}
                  <DpiVisualizer dpi={currentDpi} onChange={handleDpiChange} />

                  {/* Flex Mode Options Toggles */}
                  <div className="grid grid-cols-2 gap-3">
                    <div className="flex items-center justify-between rounded-xl border border-[#1e2330] bg-[#12151d]/60 p-3">
                      <div>
                        <div className="flex items-center gap-1.5">
                          <EyeOff className="h-3.5 w-3.5 text-zinc-400" />
                          <span className="text-xs font-semibold text-zinc-200">
                            Hide Android Taskbar
                          </span>
                        </div>
                        <p className="mt-0.5 text-[10px] text-zinc-500">
                          Removes Android bottom navigation bar for a seamless PC desktop.
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
                        className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out ${
                          settings.hideVirtualTaskbar ? "bg-cyan-500" : "bg-zinc-800"
                        }`}
                      >
                        <span
                          className={`pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out ${
                            settings.hideVirtualTaskbar ? "translate-x-4" : "translate-x-0"
                          }`}
                        />
                      </button>
                    </div>

                    <div className="flex items-center justify-between rounded-xl border border-[#1e2330] bg-[#12151d]/60 p-3">
                      <div>
                        <div className="flex items-center gap-1.5">
                          <Sun className="h-3.5 w-3.5 text-zinc-400" />
                          <span className="text-xs font-semibold text-zinc-200">
                            Turn Phone Screen Off
                          </span>
                        </div>
                        <p className="mt-0.5 text-[10px] text-zinc-500">
                          Saves phone battery & prevents OLED burn-in while mirroring.
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
                        className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out ${
                          settings.turnScreenOff ? "bg-cyan-500" : "bg-zinc-800"
                        }`}
                      >
                        <span
                          className={`pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out ${
                            settings.turnScreenOff ? "translate-x-4" : "translate-x-0"
                          }`}
                        />
                      </button>
                    </div>
                  </div>
                </div>
              )}

              {currentMode === "mirror" && (
                <div className="space-y-3">
                  <span className="font-mono text-[11px] font-bold uppercase tracking-wider text-zinc-400">
                    PHONE MIRROR TUNING
                  </span>
                  <div className="grid grid-cols-2 gap-3">
                    <div className="flex items-center justify-between rounded-xl border border-[#1e2330] bg-[#12151d]/60 p-3">
                      <div>
                        <div className="flex items-center gap-1.5">
                          <MousePointer2 className="h-3.5 w-3.5 text-zinc-400" />
                          <span className="text-xs font-semibold text-zinc-200">Show Touches</span>
                        </div>
                        <p className="mt-0.5 text-[10px] text-zinc-500">
                          Draw visual touch circles on phone screen.
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
                        className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out ${
                          settings.showTouches ? "bg-cyan-500" : "bg-zinc-800"
                        }`}
                      >
                        <span
                          className={`pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out ${
                            settings.showTouches ? "translate-x-4" : "translate-x-0"
                          }`}
                        />
                      </button>
                    </div>

                    <div className="flex items-center justify-between rounded-xl border border-[#1e2330] bg-[#12151d]/60 p-3">
                      <div>
                        <div className="flex items-center gap-1.5">
                          <Sun className="h-3.5 w-3.5 text-zinc-400" />
                          <span className="text-xs font-semibold text-zinc-200">
                            Physical Screen Off
                          </span>
                        </div>
                        <p className="mt-0.5 text-[10px] text-zinc-500">
                          Darken device display during mirror.
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
                        className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out ${
                          settings.turnScreenOff ? "bg-cyan-500" : "bg-zinc-800"
                        }`}
                      >
                        <span
                          className={`pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out ${
                            settings.turnScreenOff ? "translate-x-4" : "translate-x-0"
                          }`}
                        />
                      </button>
                    </div>
                  </div>
                </div>
              )}

              {currentMode === "camera" && (
                <div className="rounded-xl border border-[#1e2330] bg-[#12151d]/60 p-4">
                  <div className="flex items-center gap-2">
                    <CheckCircle2 className="h-4 w-4 text-amber-400" />
                    <span className="text-xs font-semibold text-zinc-200">
                      High-Definition Camera Studio Ready
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-zinc-400 leading-relaxed">
                    Uses Android camera streams for Discord, OBS, or Zoom. Audio capture is
                    muted by default to avoid echo feedback loops.
                  </p>
                </div>
              )}

              {currentMode === "hifi" && (
                <div className="rounded-xl border border-[#1e2330] bg-[#12151d]/60 p-4">
                  <div className="flex items-center gap-2">
                    <Sparkles className="h-4 w-4 text-purple-400" />
                    <span className="text-xs font-semibold text-zinc-200">
                      AV1 / HEVC High-Bitrate Pipeline (20 Mbps · 60 FPS)
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-zinc-400 leading-relaxed">
                    Calibrated for crystal sharp graphics, mobile gaming and typography. Ensure
                    a 5 GHz Wi-Fi or USB connection for best stability.
                  </p>
                </div>
              )}

              {/* Bottom Telemetry Bar & Master Trigger Button */}
              <div className="rounded-xl border border-[#1e2330] bg-[#0e1119] p-4 shadow-xl space-y-3">
                <div className="flex items-center justify-between text-[11px] font-mono text-zinc-400">
                  <div className="flex items-center gap-2">
                    <span className="text-zinc-500">ACTIVE TARGET:</span>
                    <span className="text-zinc-200 font-bold">{selectedSerial || "None"}</span>
                    <span>·</span>
                    <span className="text-cyan-400">
                      {currentMode === "flex"
                        ? `DESKTOP FLEX (${currentDpi} DPI)`
                        : currentMode.toUpperCase()}
                    </span>
                  </div>
                  <div className="text-zinc-500">CONTAINER: EMBEDDED SINGLE-WINDOW</div>
                </div>

                {activeSession ? (
                  <button
                    type="button"
                    disabled={launching}
                    onClick={handlePrimaryLaunch}
                    className="w-full flex items-center justify-center gap-2.5 rounded-xl bg-rose-600/90 py-3.5 text-sm font-bold text-white shadow-[0_0_25px_rgba(225,29,72,0.3)] hover:bg-rose-500 active:scale-[0.99] transition-all cursor-pointer"
                  >
                    <Square className="h-4 w-4 fill-white" />
                    <span>STOP ACTIVE MIRROR STREAM</span>
                  </button>
                ) : (
                  <button
                    type="button"
                    disabled={launching || !selectedSerial}
                    onClick={handlePrimaryLaunch}
                    className="w-full flex items-center justify-center gap-2.5 rounded-xl bg-gradient-to-r from-cyan-500 to-sky-500 py-3.5 text-sm font-bold text-zinc-950 shadow-[0_0_30px_rgba(56,189,248,0.35)] hover:from-cyan-400 hover:to-sky-400 active:scale-[0.99] transition-all cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <Play className="h-4 w-4 fill-zinc-950" />
                    <span>
                      {currentMode === "camera"
                        ? "CONFIGURE & LAUNCH CAMERA STUDIO"
                        : "LAUNCH EMBEDDED WORKSTATION"}
                    </span>
                  </button>
                )}
              </div>
            </>
          )}
        </div>
      </div>

      {/* Wireless Pair Modal Dialog */}
      {pairOpen && <PairDialog onClose={() => setPairOpen(false)} />}
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
