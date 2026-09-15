import { useState, useEffect } from "react";
import {
  Usb,
  Wifi,
  Battery,
  BatteryCharging,
  Pencil,
  Check,
  X,
  Trash2,
  Unplug,
  Play,
  Square,
  RefreshCw,
} from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import {
  connectDevice,
  disconnectDevice,
  discoverWireless,
  listDevices,
  startMirror,
  stopMirror,
  getBatteryInfo,
} from "@/lib/tauri";
import { forgetSaved } from "@/lib/savedDevices";
import { buildRows, deviceStatusOf } from "@/lib/deviceStatus";
import { saveNickname } from "@/lib/settingsStore";
import type { SavedDevice } from "@/lib/types";

interface DeviceRackProps {
  selectedSerial: string | null;
  onSelectDevice: (serial: string) => void;
}

interface BatteryState {
  level: number | null;
  charging: boolean;
}

export function DeviceRack({ selectedSerial, onSelectDevice }: DeviceRackProps) {
  const devices = useAppStore((s) => s.devices);
  const savedDevices = useAppStore((s) => s.savedDevices);
  const sessions = useAppStore((s) => s.sessions);
  const setDevices = useAppStore((s) => s.setDevices);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);
  const settings = useAppStore((s) => s.settings);
  const nicknames = useAppStore((s) => s.nicknames);
  const setNickname = useAppStore((s) => s.setNickname);

  const [batteryMap, setBatteryMap] = useState<Record<string, BatteryState>>({});
  const [editingSerial, setEditingSerial] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [busySerial, setBusySerial] = useState<string | null>(null);

  const { savedRows, otherDevices } = buildRows(devices, savedDevices, sessions);

  // Poll battery telemetry for connected devices
  useEffect(() => {
    let cancelled = false;
    const fetchBatteries = async () => {
      for (const dev of devices) {
        if (dev.state === "device") {
          try {
            const info = await getBatteryInfo(dev.serial);
            if (!cancelled) {
              setBatteryMap((prev) => ({ ...prev, [dev.serial]: info }));
            }
          } catch {
            // best-effort
          }
        }
      }
    };

    fetchBatteries();
    const timer = setInterval(fetchBatteries, 15000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [devices]);

  // If no device is selected yet and devices exist, pick the first available
  useEffect(() => {
    if (!selectedSerial) {
      if (devices.length > 0) {
        onSelectDevice(devices[0].serial);
      } else if (savedDevices.length > 0) {
        onSelectDevice(savedDevices[0].lastSerial || `${savedDevices[0].host}:${savedDevices[0].port}`);
      }
    }
  }, [devices, savedDevices, selectedSerial, onSelectDevice]);

  const handleStartNicknameEdit = (serial: string, currentVal: string) => {
    setEditingSerial(serial);
    setEditingName(currentVal);
  };

  const handleSaveNickname = async (serial: string) => {
    try {
      await saveNickname(serial, editingName);
      setNickname(serial, editingName);
    } catch (e) {
      setError(String(e));
    } finally {
      setEditingSerial(null);
    }
  };

  const handleConnect = async (saved: SavedDevice) => {
    setBusySerial(saved.id);
    setError(null);
    try {
      const services = await discoverWireless().catch(() => []);
      const svc = services.find((s) => s.host === saved.host && s.serviceType.includes("connect"));
      await connectDevice(saved.host, svc?.port ?? saved.port);
      setDevices(await listDevices());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusySerial(null);
    }
  };

  const handleDisconnect = async (serial: string) => {
    const [host, port] = serial.split(":");
    if (!host || !port) return;
    setBusySerial(serial);
    setError(null);
    try {
      await disconnectDevice(host, Number(port));
      setDevices(await listDevices());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusySerial(null);
    }
  };

  const handleForget = async (id: string) => {
    setBusySerial(id);
    try {
      setSavedDevices(await forgetSaved(id));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusySerial(null);
    }
  };

  const handleMirror = async (serial: string) => {
    setBusySerial(serial);
    setError(null);
    try {
      await startMirror(serial, settings);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusySerial(null);
    }
  };

  const handleStop = async (sessionId: string, serial: string) => {
    setBusySerial(serial);
    try {
      await stopMirror(sessionId);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusySerial(null);
    }
  };

  return (
    <div className="flex h-full w-80 shrink-0 flex-col border-r border-[#1e2330] bg-[#12151d]/90 select-none">
      {/* Rack Title Header */}
      <div className="flex items-center justify-between border-b border-[#1e2330] px-4 py-3 bg-[#0d1017]/80">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[11px] font-bold uppercase tracking-wider text-cyan-400">
            DEVICE RACK
          </span>
          <span className="rounded bg-[#1e2330] px-1.5 py-0.2 text-[10px] font-mono text-zinc-400">
            {devices.length + savedRows.filter((r) => !r.device).length}
          </span>
        </div>
        <span className="font-mono text-[10px] text-zinc-500">USB / WI-FI</span>
      </div>

      {/* Device Rack Items Scroll */}
      <div className="flex-1 overflow-y-auto p-3 space-y-2.5">
        {savedRows.length === 0 && otherDevices.length === 0 && (
          <div className="rounded-lg border border-dashed border-[#1e2330] p-4 text-center">
            <p className="text-xs text-zinc-400 font-medium">No devices detected</p>
            <p className="mt-1 text-[11px] text-zinc-500">
              Connect via USB with USB Debugging enabled, or pair wirelessly.
            </p>
          </div>
        )}

        {/* Live connected other devices (USB or ad-hoc wireless) */}
        {otherDevices.map((dev) => {
          const isSelected = selectedSerial === dev.serial;
          const isBusy = busySerial === dev.serial;
          const wireless = dev.serial.includes(":");
          const status = deviceStatusOf(dev, sessions);
          const session = sessions.find((s) => s.serial === dev.serial);
          const nickname = nicknames[dev.serial];
          const batt = batteryMap[dev.serial];

          return (
            <div
              key={dev.serial}
              onClick={() => onSelectDevice(dev.serial)}
              className={`group relative rounded-xl border p-3 transition-all cursor-pointer ${
                isSelected
                  ? "border-cyan-500/80 bg-[#161a26] shadow-[0_0_15px_rgba(56,189,248,0.12)] ring-1 ring-cyan-500/40"
                  : "border-[#1e2330] bg-[#141722]/70 hover:border-zinc-700 hover:bg-[#161a26]/70"
              }`}
            >
              {/* Header: Status Beacon & Hardware Identity */}
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-2">
                  <div
                    className={`h-2.5 w-2.5 rounded-full shrink-0 ${
                      status === "mirroring"
                        ? "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.8)] animate-pulse"
                        : status === "connected"
                        ? "bg-emerald-500"
                        : "bg-rose-500"
                    }`}
                  />
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5">
                      <span className="font-semibold text-xs text-zinc-100 truncate">
                        {dev.model || dev.product || dev.serial}
                      </span>
                    </div>

                    {/* Nickname inline display / edit */}
                    {editingSerial === dev.serial ? (
                      <div
                        className="mt-1 flex items-center gap-1"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <input
                          type="text"
                          className="h-6 rounded bg-zinc-800 px-1.5 text-xs text-zinc-200 border border-cyan-500/50 outline-none w-28"
                          value={editingName}
                          autoFocus
                          onChange={(e) => setEditingName(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") handleSaveNickname(dev.serial);
                            if (e.key === "Escape") setEditingSerial(null);
                          }}
                        />
                        <button
                          type="button"
                          onClick={() => handleSaveNickname(dev.serial)}
                          className="text-cyan-400 hover:text-cyan-300 p-0.5"
                        >
                          <Check className="h-3 w-3" />
                        </button>
                        <button
                          type="button"
                          onClick={() => setEditingSerial(null)}
                          className="text-zinc-500 hover:text-zinc-400 p-0.5"
                        >
                          <X className="h-3 w-3" />
                        </button>
                      </div>
                    ) : (
                      <div className="flex items-center gap-1 mt-0.5">
                        <span className="text-[11px] text-zinc-400 truncate">
                          {nickname ? `"${nickname}"` : dev.serial}
                        </span>
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleStartNicknameEdit(dev.serial, nickname || "");
                          }}
                          className="opacity-0 group-hover:opacity-100 transition-opacity text-zinc-500 hover:text-zinc-300"
                        >
                          <Pencil className="h-2.5 w-2.5" />
                        </button>
                      </div>
                    )}
                  </div>
                </div>

                {/* Transport Badge */}
                <div className="flex items-center gap-1 rounded bg-[#0d1017] px-1.5 py-0.5 border border-[#1e2330] text-[10px] font-mono text-zinc-400 shrink-0">
                  {wireless ? (
                    <>
                      <Wifi className="h-3 w-3 text-cyan-400" />
                      <span>Wi-Fi</span>
                    </>
                  ) : (
                    <>
                      <Usb className="h-3 w-3 text-zinc-400" />
                      <span>USB</span>
                    </>
                  )}
                </div>
              </div>

              {/* Hardware Telemetry Bar */}
              <div className="mt-2.5 flex items-center justify-between border-t border-[#1e2330]/70 pt-2 text-[10px] text-zinc-400 font-mono">
                <span className="truncate max-w-[120px]">{dev.serial}</span>
                {batt?.level !== null && batt?.level !== undefined && (
                  <div className="flex items-center gap-1 text-zinc-300">
                    {batt.charging ? (
                      <BatteryCharging className="h-3 w-3 text-emerald-400" />
                    ) : (
                      <Battery className="h-3 w-3 text-zinc-400" />
                    )}
                    <span>{batt.level}%</span>
                  </div>
                )}
              </div>

              {/* Quick Actions Row */}
              <div
                className="mt-2.5 flex items-center gap-1.5"
                onClick={(e) => e.stopPropagation()}
              >
                {status === "mirroring" && session ? (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleStop(session.id, dev.serial)}
                    className="flex-1 flex items-center justify-center gap-1 rounded bg-rose-500/20 py-1 text-[11px] font-medium text-rose-300 border border-rose-500/30 hover:bg-rose-500/30 active:scale-[0.98] transition-all"
                  >
                    <Square className="h-3 w-3 fill-rose-300" />
                    Stop
                  </button>
                ) : (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleMirror(dev.serial)}
                    className="flex-1 flex items-center justify-center gap-1 rounded bg-cyan-500/20 py-1 text-[11px] font-medium text-cyan-300 border border-cyan-500/30 hover:bg-cyan-500/30 active:scale-[0.98] transition-all"
                  >
                    <Play className="h-3 w-3 fill-cyan-300" />
                    Launch
                  </button>
                )}

                {wireless && (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleDisconnect(dev.serial)}
                    className="rounded bg-zinc-800 p-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 active:scale-[0.98] transition-all"
                    title="Disconnect wireless"
                  >
                    <Unplug className="h-3.5 w-3.5" />
                  </button>
                )}
              </div>
            </div>
          );
        })}

        {/* Saved wireless devices */}
        {savedRows.map((r) => {
          const { saved, device, status } = r;
          const serial = device?.serial;
          const targetSerial = serial ?? `${saved.host}:${saved.port}`;
          const isSelected = selectedSerial === targetSerial || (serial && selectedSerial === serial);
          const isBusy = busySerial === saved.id || Boolean(serial && busySerial === serial);
          const session = serial ? sessions.find((s) => s.serial === serial) : undefined;
          const nickname = nicknames[targetSerial] ?? (serial ? nicknames[serial] : undefined);
          const batt = serial ? batteryMap[serial] : undefined;

          return (
            <div
              key={saved.id}
              onClick={() => onSelectDevice(targetSerial)}
              className={`group relative rounded-xl border p-3 transition-all cursor-pointer ${
                isSelected
                  ? "border-cyan-500/80 bg-[#161a26] shadow-[0_0_15px_rgba(56,189,248,0.12)] ring-1 ring-cyan-500/40"
                  : status === "offline"
                  ? "border-[#1e2330]/70 bg-[#12151d]/50 opacity-80 hover:opacity-100 hover:border-zinc-700"
                  : "border-[#1e2330] bg-[#141722]/70 hover:border-zinc-700 hover:bg-[#161a26]/70"
              }`}
            >
              {/* Header */}
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-2">
                  <div
                    className={`h-2.5 w-2.5 rounded-full shrink-0 ${
                      status === "mirroring"
                        ? "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.8)] animate-pulse"
                        : status === "connected"
                        ? "bg-emerald-500"
                        : "bg-zinc-600"
                    }`}
                  />
                  <div className="min-w-0">
                    <span className="font-semibold text-xs text-zinc-100 truncate block">
                      {saved.label || targetSerial}
                    </span>

                    {/* Nickname inline display / edit */}
                    {editingSerial === targetSerial ? (
                      <div
                        className="mt-1 flex items-center gap-1"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <input
                          type="text"
                          className="h-6 rounded bg-zinc-800 px-1.5 text-xs text-zinc-200 border border-cyan-500/50 outline-none w-28"
                          value={editingName}
                          autoFocus
                          onChange={(e) => setEditingName(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") handleSaveNickname(targetSerial);
                            if (e.key === "Escape") setEditingSerial(null);
                          }}
                        />
                        <button
                          type="button"
                          onClick={() => handleSaveNickname(targetSerial)}
                          className="text-cyan-400 hover:text-cyan-300 p-0.5"
                        >
                          <Check className="h-3 w-3" />
                        </button>
                        <button
                          type="button"
                          onClick={() => setEditingSerial(null)}
                          className="text-zinc-500 hover:text-zinc-400 p-0.5"
                        >
                          <X className="h-3 w-3" />
                        </button>
                      </div>
                    ) : (
                      <div className="flex items-center gap-1 mt-0.5">
                        <span className="text-[11px] text-zinc-400 truncate">
                          {nickname ? `"${nickname}"` : targetSerial}
                        </span>
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleStartNicknameEdit(targetSerial, nickname || "");
                          }}
                          className="opacity-0 group-hover:opacity-100 transition-opacity text-zinc-500 hover:text-zinc-300"
                        >
                          <Pencil className="h-2.5 w-2.5" />
                        </button>
                      </div>
                    )}
                  </div>
                </div>

                {/* Wireless Badge */}
                <div className="flex items-center gap-1 rounded bg-[#0d1017] px-1.5 py-0.5 border border-[#1e2330] text-[10px] font-mono text-cyan-400 shrink-0">
                  <Wifi className="h-3 w-3" />
                  <span>Wi-Fi</span>
                </div>
              </div>

              {/* Hardware Telemetry Bar */}
              <div className="mt-2.5 flex items-center justify-between border-t border-[#1e2330]/70 pt-2 text-[10px] text-zinc-400 font-mono">
                <span className="truncate max-w-[120px]">
                  {status === "offline" ? "Saved Device" : serial}
                </span>
                {batt?.level !== null && batt?.level !== undefined && (
                  <div className="flex items-center gap-1 text-zinc-300">
                    {batt.charging ? (
                      <BatteryCharging className="h-3 w-3 text-emerald-400" />
                    ) : (
                      <Battery className="h-3 w-3 text-zinc-400" />
                    )}
                    <span>{batt.level}%</span>
                  </div>
                )}
              </div>

              {/* Quick Actions */}
              <div
                className="mt-2.5 flex items-center gap-1.5"
                onClick={(e) => e.stopPropagation()}
              >
                {status === "offline" ? (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleConnect(saved)}
                    className="flex-1 flex items-center justify-center gap-1 rounded bg-zinc-800 py-1 text-[11px] font-medium text-zinc-200 border border-zinc-700 hover:bg-zinc-700 active:scale-[0.98] transition-all"
                  >
                    <RefreshCw className={`h-3 w-3 ${isBusy ? "animate-spin" : ""}`} />
                    Connect
                  </button>
                ) : status === "mirroring" && session ? (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleStop(session.id, targetSerial)}
                    className="flex-1 flex items-center justify-center gap-1 rounded bg-rose-500/20 py-1 text-[11px] font-medium text-rose-300 border border-rose-500/30 hover:bg-rose-500/30 active:scale-[0.98] transition-all"
                  >
                    <Square className="h-3 w-3 fill-rose-300" />
                    Stop
                  </button>
                ) : (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => serial && handleMirror(serial)}
                    className="flex-1 flex items-center justify-center gap-1 rounded bg-cyan-500/20 py-1 text-[11px] font-medium text-cyan-300 border border-cyan-500/30 hover:bg-cyan-500/30 active:scale-[0.98] transition-all"
                  >
                    <Play className="h-3 w-3 fill-cyan-300" />
                    Launch
                  </button>
                )}

                {status !== "offline" && serial && (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleDisconnect(serial)}
                    className="rounded bg-zinc-800 p-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 active:scale-[0.98] transition-all"
                    title="Disconnect wireless"
                  >
                    <Unplug className="h-3.5 w-3.5" />
                  </button>
                )}

                <button
                  type="button"
                  disabled={isBusy}
                  onClick={() => handleForget(saved.id)}
                  className="rounded bg-zinc-800 p-1 text-zinc-500 hover:text-rose-400 hover:bg-zinc-700 active:scale-[0.98] transition-all"
                  title="Forget device"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
