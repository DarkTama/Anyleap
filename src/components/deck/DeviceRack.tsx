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
import {
  buildRows,
  deviceStatusOf,
  resolveDeviceNickname,
  getNicknameKey,
} from "@/lib/deviceStatus";
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

  const handleSaveNickname = async (primaryKey: string, secondaryKey?: string) => {
    try {
      await saveNickname(primaryKey, editingName);
      setNickname(primaryKey, editingName);
      if (secondaryKey && secondaryKey !== primaryKey) {
        await saveNickname(secondaryKey, editingName);
        setNickname(secondaryKey, editingName);
      }
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
    <div className="flex h-full w-72 shrink-0 flex-col border-r border-[#222226] bg-[#0e0e11] select-none">
      {/* Rack Title Header */}
      <div className="flex items-center justify-between border-b border-[#222226] px-4 py-3 bg-[#0e0e11]">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-zinc-400">
            Device Rack
          </span>
          <span className="rounded bg-[#1a1a20] px-1.5 py-0.5 text-[9px] font-mono text-zinc-400 border border-[#27272a]">
            {devices.length + savedRows.filter((r) => !r.device).length}
          </span>
        </div>
        <span className="font-mono text-[10px] text-zinc-500">USB / WI-FI</span>
      </div>

      {/* Device Rack Items Scroll */}
      <div className="flex-1 overflow-y-auto p-3 space-y-2.5">
        {savedRows.length === 0 && otherDevices.length === 0 && (
          <div className="rounded-xl border border-dashed border-[#222226] p-5 text-center">
            <p className="text-xs text-zinc-300 font-medium">No devices detected</p>
            <p className="mt-1 text-[11px] text-zinc-500 leading-relaxed">
              Connect via USB with USB Debugging enabled, or click Pair Device.
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
              className={`btn-tactile group relative rounded-xl border p-3 cursor-pointer ${
                isSelected
                  ? "border-[#52525b] bg-[#1a1a20] shadow-sm"
                  : "border-[#222226] bg-[#131316] text-zinc-400 hover:border-zinc-700 hover:bg-[#16161a]"
              }`}
            >
              {/* Header: Status Beacon & Hardware Identity */}
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-2 min-w-0">
                  <div
                    className={`h-2 w-2 rounded-full shrink-0 ${
                      status === "mirroring"
                        ? "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.8)] animate-pulse"
                        : status === "connected"
                        ? "bg-emerald-500"
                        : "bg-rose-500"
                    }`}
                  />
                  <div className="min-w-0">
                    <span className="font-semibold text-xs text-zinc-100 truncate block">
                      {dev.model || dev.product || dev.serial}
                    </span>

                    {/* Nickname inline display / edit */}
                    {editingSerial === dev.serial ? (
                      <div
                        className="mt-1 flex items-center gap-1"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <input
                          type="text"
                          className="h-6 rounded bg-[#18181c] px-1.5 text-xs text-zinc-200 border border-zinc-500 outline-none w-28"
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
                          className="text-zinc-200 hover:text-white p-0.5"
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
                <div className="flex items-center gap-1 rounded bg-[#18181c] px-1.5 py-0.5 border border-[#27272a] text-[10px] font-mono text-zinc-400 shrink-0">
                  {wireless ? (
                    <>
                      <Wifi className="h-3 w-3 text-zinc-300" />
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
              <div className="mt-2.5 flex items-center justify-between border-t border-[#222226] pt-2 text-[10px] text-zinc-400 font-mono">
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
                    className="btn-tactile flex-1 flex items-center justify-center gap-1 rounded-md bg-rose-500/15 py-1 text-[11px] font-medium text-rose-300 border border-rose-500/30 hover:bg-rose-500/25"
                  >
                    <Square className="h-3 w-3 fill-rose-300" />
                    Stop
                  </button>
                ) : (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleMirror(dev.serial)}
                    className="btn-tactile flex-1 flex items-center justify-center gap-1 rounded-md bg-[#222228] py-1 text-[11px] font-medium text-zinc-200 border border-[#2e2e36] hover:bg-[#282830]"
                  >
                    <Play className="h-3 w-3 fill-zinc-300" />
                    Launch
                  </button>
                )}

                {wireless && (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleDisconnect(dev.serial)}
                    className="btn-tactile rounded-md bg-[#18181c] p-1 text-zinc-400 border border-[#27272a] hover:text-zinc-200 hover:bg-[#202026]"
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
          const nickname = resolveDeviceNickname(nicknames, { serial, saved });
          const nicknameKey = getNicknameKey({ serial, saved });
          const batt = serial ? batteryMap[serial] : undefined;

          return (
            <div
              key={saved.id}
              onClick={() => onSelectDevice(targetSerial)}
              className={`btn-tactile group relative rounded-xl border p-3 cursor-pointer ${
                isSelected
                  ? "border-[#52525b] bg-[#1a1a20] shadow-sm"
                  : status === "offline"
                  ? "border-[#222226]/60 bg-[#111114]/50 opacity-70 hover:opacity-100 hover:border-zinc-700"
                  : "border-[#222226] bg-[#131316] text-zinc-400 hover:border-zinc-700 hover:bg-[#16161a]"
              }`}
            >
              {/* Header */}
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-2 min-w-0">
                  <div
                    className={`h-2 w-2 rounded-full shrink-0 ${
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
                          className="h-6 rounded bg-[#18181c] px-1.5 text-xs text-zinc-200 border border-zinc-500 outline-none w-28"
                          value={editingName}
                          autoFocus
                          onChange={(e) => setEditingName(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") handleSaveNickname(nicknameKey, serial);
                            if (e.key === "Escape") setEditingSerial(null);
                          }}
                        />
                        <button
                          type="button"
                          onClick={() => handleSaveNickname(nicknameKey, serial)}
                          className="text-zinc-200 hover:text-white p-0.5"
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
                <div className="flex items-center gap-1 rounded bg-[#18181c] px-1.5 py-0.5 border border-[#27272a] text-[10px] font-mono text-zinc-400 shrink-0">
                  <Wifi className="h-3 w-3 text-zinc-300" />
                  <span>Wi-Fi</span>
                </div>
              </div>

              {/* Hardware Telemetry Bar */}
              <div className="mt-2.5 flex items-center justify-between border-t border-[#222226] pt-2 text-[10px] text-zinc-400 font-mono">
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
                    className="btn-tactile flex-1 flex items-center justify-center gap-1 rounded-md bg-[#18181c] py-1 text-[11px] font-medium text-zinc-200 border border-[#27272a] hover:bg-[#202026]"
                  >
                    <RefreshCw className={`h-3 w-3 ${isBusy ? "animate-spin" : ""}`} />
                    Connect
                  </button>
                ) : status === "mirroring" && session ? (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleStop(session.id, targetSerial)}
                    className="btn-tactile flex-1 flex items-center justify-center gap-1 rounded-md bg-rose-500/15 py-1 text-[11px] font-medium text-rose-300 border border-rose-500/30 hover:bg-rose-500/25"
                  >
                    <Square className="h-3 w-3 fill-rose-300" />
                    Stop
                  </button>
                ) : (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => serial && handleMirror(serial)}
                    className="btn-tactile flex-1 flex items-center justify-center gap-1 rounded-md bg-[#222228] py-1 text-[11px] font-medium text-zinc-200 border border-[#2e2e36] hover:bg-[#282830]"
                  >
                    <Play className="h-3 w-3 fill-zinc-300" />
                    Launch
                  </button>
                )}

                {status !== "offline" && serial && (
                  <button
                    type="button"
                    disabled={isBusy}
                    onClick={() => handleDisconnect(serial)}
                    className="btn-tactile rounded-md bg-[#18181c] p-1 text-zinc-400 border border-[#27272a] hover:text-zinc-200 hover:bg-[#202026]"
                    title="Disconnect wireless"
                  >
                    <Unplug className="h-3.5 w-3.5" />
                  </button>
                )}

                <button
                  type="button"
                  disabled={isBusy}
                  onClick={() => handleForget(saved.id)}
                  className="btn-tactile rounded-md bg-[#18181c] p-1 text-zinc-500 border border-[#27272a] hover:text-rose-400 hover:bg-[#202026]"
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
