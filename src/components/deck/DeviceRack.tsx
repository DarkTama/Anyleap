import { useState, useEffect, useRef } from "react";
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
  Stethoscope,
  Link2,
  Phone,
  Send,
} from "lucide-react";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { useAppStore } from "@/store/useAppStore";
import { logActivity } from "@/stores/activityStore";
import {
  connectDevice,
  disconnectDevice,
  discoverWireless,
  listDevices,
  startMirror,
  stopMirror,
  getBatteryInfo,
  openUrlOnDevice,
  placeCall,
} from "@/lib/tauri";
import { nextBatteryAlert, type BatteryAlertKind } from "@/lib/batteryAlerts";
import { forgetSaved } from "@/lib/savedDevices";
import {
  buildRows,
  deviceStatusOf,
  resolveDeviceNickname,
  getNicknameKey,
} from "@/lib/deviceStatus";
import { saveNickname } from "@/lib/settingsStore";
import type { BatteryInfo, SavedDevice } from "@/lib/types";

interface DeviceRackProps {
  selectedSerial: string | null;
  onSelectDevice: (serial: string) => void;
}

export function DeviceRack({ selectedSerial, onSelectDevice }: DeviceRackProps) {
  const devices = useAppStore((s) => s.devices);
  const savedDevices = useAppStore((s) => s.savedDevices);
  const sessions = useAppStore((s) => s.sessions);
  const setDevices = useAppStore((s) => s.setDevices);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);
  const settings = useAppStore((s) => s.settings);
  const appPrefs = useAppStore((s) => s.appPrefs);
  const nicknames = useAppStore((s) => s.nicknames);
  const setNickname = useAppStore((s) => s.setNickname);
  const batteryMap = useAppStore((s) => s.batteryMap);
  const setBatteryInfo = useAppStore((s) => s.setBatteryInfo);
  const openDoctor = useAppStore((s) => s.openDoctor);

  const prevBatteryRef = useRef<Record<string, BatteryInfo>>({});
  const [editingSerial, setEditingSerial] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [busySerial, setBusySerial] = useState<string | null>(null);
  const [failedSerial, setFailedSerial] = useState<string | null>(null);
  const [quickAction, setQuickAction] = useState<{ serial: string; kind: "url" | "call" } | null>(null);
  const [quickInput, setQuickInput] = useState("");
  const [quickNote, setQuickNote] = useState<{ serial: string; text: string } | null>(null);

  const { savedRows, otherDevices } = buildRows(devices, savedDevices, sessions);

  const notifyBattery = async (deviceName: string, kind: BatteryAlertKind, level: number) => {
    try {
      let granted = await isPermissionGranted();
      if (!granted) {
        const perm = await requestPermission();
        granted = perm === "granted";
      }
      if (!granted) return;
      sendNotification({
        title: kind === "low" ? `${deviceName}: Battery low (${level}%)` : `${deviceName}: Battery full (100%)`,
        body:
          kind === "low"
            ? "Plug in your device to avoid disconnecting."
            : "Device is fully charged.",
      });
    } catch {
      // best-effort notification
    }
  };

  // Poll battery telemetry for connected devices
  useEffect(() => {
    let cancelled = false;
    const fetchBatteries = async () => {
      for (const dev of devices) {
        if (dev.state === "device") {
          try {
            const info = await getBatteryInfo(dev.serial);
            if (!cancelled) {
              const prev = prevBatteryRef.current[dev.serial];
              prevBatteryRef.current[dev.serial] = info;
              if (appPrefs.batteryAlerts) {
                const alert = nextBatteryAlert(prev, info);
                if (alert) {
                  void notifyBattery(dev.model || dev.product || dev.serial, alert, info.level ?? 0);
                }
              }
              setBatteryInfo(dev.serial, info);
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
  }, [devices, setBatteryInfo, appPrefs.batteryAlerts]);

  const openQuickInput = async (serial: string, kind: "url" | "call") => {
    if (quickAction?.serial === serial && quickAction.kind === kind) {
      setQuickAction(null);
      return;
    }
    setQuickNote(null);
    setQuickInput("");
    setQuickAction({ serial, kind });
    if (kind === "url") {
      try {
        const clip = (await navigator.clipboard.readText()).trim();
        if (/^(https?:\/\/|geo:|mailto:)/i.test(clip)) {
          setQuickInput(clip);
        }
      } catch {
        // clipboard may be unavailable
      }
    }
  };

  const submitQuickInput = async (serial: string) => {
    if (!quickAction || quickAction.serial !== serial) return;
    setBusySerial(serial);
    setError(null);
    try {
      if (quickAction.kind === "url") {
        await openUrlOnDevice(serial, quickInput);
        logActivity({
          kind: "device",
          title: "Opened link on phone",
          detail: quickInput.trim(),
        });
        setQuickAction(null);
      } else {
        const note = await placeCall(serial, quickInput);
        logActivity({
          kind: "device",
          title: "Call placed",
          detail: note ? `${quickInput.trim()} (${note})` : quickInput.trim(),
        });
        setQuickAction(null);
        if (note) {
          setQuickNote({ serial, text: note });
        }
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusySerial(null);
    }
  };

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
    setFailedSerial(null);
    try {
      const services = await discoverWireless().catch(() => []);
      const svc = services.find((s) => s.host === saved.host && s.serviceType.includes("connect"));
      const connectedSerial = await connectDevice(saved.host, svc?.port ?? saved.port);
      setDevices(await listDevices());
      logActivity({
        kind: "device",
        title: `Connected ${saved.label || connectedSerial}`,
        detail: connectedSerial,
      });
    } catch (e) {
      setFailedSerial(saved.id);
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
      logActivity({
        kind: "device",
        title: `Disconnected ${serial}`,
      });
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
    setFailedSerial(null);
    try {
      await startMirror(serial, settings);
    } catch (e) {
      const msg = String(e);
      setFailedSerial(serial);
      setError(msg);
      logActivity({
        kind: "mirror",
        title: `Failed to mirror ${serial}`,
        detail: msg,
      });
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

                {(dev.state !== "device" || failedSerial === dev.serial) && (
                  <button
                    type="button"
                    onClick={() => openDoctor(dev.serial)}
                    className="btn-tactile rounded-md bg-amber-500/15 p-1 text-amber-300 border border-amber-500/30 hover:bg-amber-500/25"
                    title="Run Connection Doctor"
                  >
                    <Stethoscope className="h-3.5 w-3.5" />
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
                <button
                  type="button"
                  disabled={isBusy}
                  onClick={() => void openQuickInput(dev.serial, "url")}
                  className={`btn-tactile rounded-md p-1 border ${
                    quickAction?.serial === dev.serial && quickAction.kind === "url"
                      ? "bg-[#282830] text-zinc-100 border-zinc-500"
                      : "bg-[#18181c] text-zinc-400 border-[#27272a] hover:text-zinc-200 hover:bg-[#202026]"
                  }`}
                  title="Open link on phone"
                >
                  <Link2 className="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  disabled={isBusy}
                  onClick={() => void openQuickInput(dev.serial, "call")}
                  className={`btn-tactile rounded-md p-1 border ${
                    quickAction?.serial === dev.serial && quickAction.kind === "call"
                      ? "bg-[#282830] text-zinc-100 border-zinc-500"
                      : "bg-[#18181c] text-zinc-400 border-[#27272a] hover:text-zinc-200 hover:bg-[#202026]"
                  }`}
                  title="Call…"
                >
                  <Phone className="h-3.5 w-3.5" />
                </button>
              </div>
              {quickAction?.serial === dev.serial && (
                <div
                  className="mt-2 flex items-center gap-1"
                  onClick={(e) => e.stopPropagation()}
                >
                  <input
                    type={quickAction.kind === "call" ? "tel" : "url"}
                    value={quickInput}
                    autoFocus
                    onChange={(e) => setQuickInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void submitQuickInput(dev.serial);
                      if (e.key === "Escape") setQuickAction(null);
                    }}
                    placeholder={quickAction.kind === "url" ? "https://…" : "+1 555 123 4567"}
                    className="h-7 min-w-0 flex-1 rounded-md border border-[#2e2e36] bg-[#18181c] px-2 text-[11px] text-zinc-200 placeholder:text-zinc-500 focus:border-zinc-500 focus:outline-none"
                  />
                  <button
                    type="button"
                    disabled={isBusy || !quickInput.trim()}
                    onClick={() => void submitQuickInput(dev.serial)}
                    className="btn-tactile rounded-md border border-[#2e2e36] bg-[#222228] p-1.5 text-zinc-200 hover:bg-[#282830] disabled:opacity-40"
                    title={quickAction.kind === "url" ? "Open on phone" : "Place call"}
                  >
                    <Send className="h-3 w-3" />
                  </button>
                </div>
              )}
              {quickNote?.serial === dev.serial && (
                <div className="mt-1.5 text-[10px] text-amber-300">
                  {quickNote.text}
                </div>
              )}
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

                {(failedSerial === saved.id || (serial && failedSerial === serial) || (device && device.state !== "device")) && (
                  <button
                    type="button"
                    onClick={() => openDoctor(targetSerial)}
                    className="btn-tactile rounded-md bg-amber-500/15 p-1 text-amber-300 border border-amber-500/30 hover:bg-amber-500/25"
                    title="Run Connection Doctor"
                  >
                    <Stethoscope className="h-3.5 w-3.5" />
                  </button>
                )}

                {status !== "offline" && serial && (
                  <>
                    <button
                      type="button"
                      disabled={isBusy}
                      onClick={() => handleDisconnect(serial)}
                      className="btn-tactile rounded-md bg-[#18181c] p-1 text-zinc-400 border border-[#27272a] hover:text-zinc-200 hover:bg-[#202026]"
                      title="Disconnect wireless"
                    >
                      <Unplug className="h-3.5 w-3.5" />
                    </button>
                    <button
                      type="button"
                      disabled={isBusy}
                      onClick={() => void openQuickInput(serial, "url")}
                      className={`btn-tactile rounded-md p-1 border ${
                        quickAction?.serial === serial && quickAction.kind === "url"
                          ? "bg-[#282830] text-zinc-100 border-zinc-500"
                          : "bg-[#18181c] text-zinc-400 border-[#27272a] hover:text-zinc-200 hover:bg-[#202026]"
                      }`}
                      title="Open link on phone"
                    >
                      <Link2 className="h-3.5 w-3.5" />
                    </button>
                    <button
                      type="button"
                      disabled={isBusy}
                      onClick={() => void openQuickInput(serial, "call")}
                      className={`btn-tactile rounded-md p-1 border ${
                        quickAction?.serial === serial && quickAction.kind === "call"
                          ? "bg-[#282830] text-zinc-100 border-zinc-500"
                          : "bg-[#18181c] text-zinc-400 border-[#27272a] hover:text-zinc-200 hover:bg-[#202026]"
                      }`}
                      title="Call…"
                    >
                      <Phone className="h-3.5 w-3.5" />
                    </button>
                  </>
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
              {serial && quickAction?.serial === serial && (
                <div
                  className="mt-2 flex items-center gap-1"
                  onClick={(e) => e.stopPropagation()}
                >
                  <input
                    type={quickAction.kind === "call" ? "tel" : "url"}
                    value={quickInput}
                    autoFocus
                    onChange={(e) => setQuickInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void submitQuickInput(serial);
                      if (e.key === "Escape") setQuickAction(null);
                    }}
                    placeholder={quickAction.kind === "url" ? "https://…" : "+1 555 123 4567"}
                    className="h-7 min-w-0 flex-1 rounded-md border border-[#2e2e36] bg-[#18181c] px-2 text-[11px] text-zinc-200 placeholder:text-zinc-500 focus:border-zinc-500 focus:outline-none"
                  />
                  <button
                    type="button"
                    disabled={isBusy || !quickInput.trim()}
                    onClick={() => void submitQuickInput(serial)}
                    className="btn-tactile rounded-md border border-[#2e2e36] bg-[#222228] p-1.5 text-zinc-200 hover:bg-[#282830] disabled:opacity-40"
                    title={quickAction.kind === "url" ? "Open on phone" : "Place call"}
                  >
                    <Send className="h-3 w-3" />
                  </button>
                </div>
              )}
              {serial && quickNote?.serial === serial && (
                <div className="mt-1.5 text-[10px] text-amber-300">
                  {quickNote.text}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
