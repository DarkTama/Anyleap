import { useState, useEffect } from "react";
import { useAppStore } from "@/store/useAppStore";
import { logActivity } from "@/stores/activityStore";
import {
  disconnectDevice,
  startMirror,
  stopMirror,
  openUrlOnDevice,
  placeCall,
} from "@/lib/tauri";
import { forgetSaved } from "@/lib/savedDevices";
import {
  buildRows,
  deviceStatusOf,
  savedHardwareKey,
  findSessionForDevice,
  getNicknameKey,
  isWifiSerial,
  MSG_NEEDS_REPAIR,
  resolveDeviceNickname,
  resolveDeviceTarget,
} from "@/lib/deviceStatus";
import {
  reconnectSavedDevice,
  repairKeyFor,
  syncDevicesAndMigrate,
} from "@/lib/deviceSync";
import { saveNickname } from "@/lib/settingsStore";
import type { SavedDevice } from "@/lib/types";
import { useBatteryPolling } from "./rack/useBatteryPolling";
import { RackHeader } from "./rack/RackHeader";
import { RackEmptyState } from "./rack/RackEmptyState";
import { DeviceListItem, type TransportKind } from "./rack/DeviceListItem";
import { PairingEntryCard } from "./rack/PairingEntryCard";
import type { QuickActionState } from "./rack/DeviceConnectionControls";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import { resolveBatteryChipData } from "./phoneOverviewStore";

interface DeviceRackProps {
  selectedSerial: string | null;
  onSelectDevice: (serial: string) => void;
  onOpenPair?: () => void;
}

export function DeviceRack({
  selectedSerial,
  onSelectDevice,
  onOpenPair,
}: DeviceRackProps) {
  const devices = useAppStore((s) => s.devices);
  const savedDevices = useAppStore((s) => s.savedDevices);
  const sessions = useAppStore((s) => s.sessions);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);
  const settings = useAppStore((s) => s.settings);
  const appPrefs = useAppStore((s) => s.appPrefs);
  const nicknames = useAppStore((s) => s.nicknames);
  const setNickname = useAppStore((s) => s.setNickname);
  const batteryMap = useAppStore((s) => s.batteryMap);
  const setBatteryInfo = useAppStore((s) => s.setBatteryInfo);
  const overviewBySerial = usePhoneHubStore((s) => s.overviewsBySerial);
  const openDoctor = useAppStore((s) => s.openDoctor);
  const repairNeeded = useAppStore((s) => s.repairNeeded);
  const reconnectingMap = useAppStore((s) => s.reconnecting);

  const [editingSerial, setEditingSerial] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [busySerial, setBusySerial] = useState<string | null>(null);
  const [failedSerial, setFailedSerial] = useState<string | null>(null);
  const [quickAction, setQuickAction] = useState<QuickActionState | null>(null);
  const [quickInput, setQuickInput] = useState("");
  const [quickNote, setQuickNote] = useState<{ serial: string; text: string } | null>(null);

  const { savedRows, otherDevices } = buildRows(devices, savedDevices, sessions);

  useBatteryPolling(devices, setBatteryInfo, appPrefs.batteryAlerts);

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

  // If no device is selected yet and devices exist, pick the first available merged target
  useEffect(() => {
    if (!selectedSerial) {
      const first = resolveDeviceTarget(null, devices, savedDevices);
      if (first.activeSerial) {
        onSelectDevice(first.activeSerial);
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
      const res = await reconnectSavedDevice(saved, { silent: false });
      if (res.status === "needs_repair") {
        setFailedSerial(saved.id);
        return;
      }
      if (res.serial) {
        onSelectDevice(res.serial);
      }
      logActivity({
        kind: "device",
        title: `Connected ${saved.label || res.serial || saved.host}`,
        detail: res.serial ?? undefined,
        deviceId: res.hwSerial ?? saved.hw_serial ?? undefined,
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
      await syncDevicesAndMigrate(undefined, undefined, { force: true });
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

  const handleMirror = async (serial: string, saved?: SavedDevice) => {
    setBusySerial(serial);
    setError(null);
    setFailedSerial(null);
    try {
      await startMirror(serial, settings);
    } catch (e) {
      if (saved && isWifiSerial(serial)) {
        const res = await reconnectSavedDevice(saved, { silent: false });
        if (
          (res.status === "reconnected" || res.status === "already_connected") &&
          res.serial
        ) {
          try {
            await startMirror(res.serial, settings);
            return;
          } catch (retryErr) {
            const msg = String(retryErr);
            setFailedSerial(res.serial);
            setError(msg);
            return;
          }
        }
        if (res.status === "needs_repair") {
          setFailedSerial(saved.id);
          return;
        }
      }
      const msg = String(e);
      setFailedSerial(serial);
      setError(msg);
      logActivity({
        kind: "mirror",
        title: `Failed to mirror ${serial}`,
        detail: msg,
        deviceId: saved?.hw_serial ?? undefined,
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
    <div className="flex h-full w-72 shrink-0 flex-col border-r border-ink-700 bg-ink-900 select-none">
      <RackHeader deviceCount={savedRows.length + otherDevices.length} />

      <div className="flex-1 overflow-y-auto p-3 space-y-2.5">
        {savedRows.length === 0 && otherDevices.length === 0 && <RackEmptyState />}

        {/* Live connected other devices (USB or ad-hoc wireless) */}
        {otherDevices.map((dev) => {
          const isSelected = Boolean(
            selectedSerial === dev.serial ||
              (dev.hw_serial && selectedSerial === dev.hw_serial) ||
              (dev.transports?.usb && selectedSerial === dev.transports.usb) ||
              (dev.transports?.wifi && selectedSerial === dev.transports.wifi),
          );
          const isBusy = busySerial === dev.serial;
          const wireless = dev.serial.includes(":");
          const status = deviceStatusOf(dev, sessions);
          const session = findSessionForDevice({ device: dev }, sessions);
          const nickname = resolveDeviceNickname(nicknames, {
            serial: dev.serial,
            device: dev,
          });
          const nicknameKey = getNicknameKey({ serial: dev.serial, device: dev });
          const transport: TransportKind = wireless ? "wifi" : "usb";

          return (
            <DeviceListItem
              key={dev.hw_serial || dev.serial}
              title={dev.model || dev.product || dev.serial}
              subtitle={nickname ? `"${nickname}"` : dev.serial}
              telemetryLabel={dev.serial}
              status={status}
              offlineTone="danger"
              isSelected={isSelected}
              transport={transport}
              battery={resolveBatteryChipData(overviewBySerial[dev.serial], batteryMap[dev.serial])}
              isEditing={editingSerial === dev.serial}
              editingName={editingName}
              serial={dev.serial}
              hasSession={Boolean(session)}
              isBusy={isBusy}
              showDoctor={dev.state !== "device" || failedSerial === dev.serial}
              showDisconnect={wireless}
              canQuickAction={true}
              canForget={false}
              quickAction={quickAction}
              quickInput={quickInput}
              quickNote={quickNote?.serial === dev.serial ? quickNote.text : null}
              onSelect={() => onSelectDevice(dev.serial)}
              onStartEdit={() => handleStartNicknameEdit(dev.serial, nickname || "")}
              onEditingNameChange={setEditingName}
              onSaveEdit={() => void handleSaveNickname(nicknameKey, dev.serial)}
              onCancelEdit={() => setEditingSerial(null)}
              onStop={() => session && void handleStop(session.id, dev.serial)}
              onLaunch={() => void handleMirror(dev.serial)}
              onDoctor={() => openDoctor(dev.serial)}
              onDisconnect={() => void handleDisconnect(dev.serial)}
              onOpenQuickInput={(kind) => void openQuickInput(dev.serial, kind)}
              onQuickInputChange={setQuickInput}
              onSubmitQuickInput={() => void submitQuickInput(dev.serial)}
              onCloseQuickInput={() => setQuickAction(null)}
            />
          );
        })}

        {/* Saved devices */}
        {savedRows.map((r) => {
          const { saved, device, status } = r;
          const serial = device?.serial;
          const targetSerial = serial ?? (saved.lastSerial || `${saved.host}:${saved.port}`);
          const isSelected = Boolean(
            selectedSerial === targetSerial ||
              (serial && selectedSerial === serial) ||
              (saved.hw_serial && selectedSerial === saved.hw_serial) ||
              (device?.transports?.usb && selectedSerial === device.transports.usb) ||
              (device?.transports?.wifi && selectedSerial === device.transports.wifi),
          );
          const isBusy = busySerial === saved.id || Boolean(serial && busySerial === serial);
          const session = findSessionForDevice(
            { device, saved, serial: targetSerial },
            sessions,
          );
          const nickname = resolveDeviceNickname(nicknames, {
            serial,
            saved,
            device,
          });
          const nicknameKey = getNicknameKey({ serial, saved, device });
          const rKey = repairKeyFor({ device, saved, serial: targetSerial });
          const rowRepairMsg =
            status === "offline" || (device && device.state !== "device")
              ? repairNeeded[rKey] ??
                repairNeeded[saved.id] ??
                (device?.state === "unauthorized" ? MSG_NEEDS_REPAIR : null)
              : null;
          const dualTransport = Boolean(device?.transports?.usb && device?.transports?.wifi);
          const usbOnly = Boolean(
            !dualTransport && (device?.transports?.usb || (serial && !isWifiSerial(serial))),
          );
          const transport: TransportKind = dualTransport
            ? "dual"
            : usbOnly
            ? "usb"
            : "wifi";
          const telemetryLabel =
            status === "offline"
              ? rowRepairMsg
                ? MSG_NEEDS_REPAIR
                : "Saved Device"
              : serial ?? targetSerial;

          return (
            <DeviceListItem
              key={saved.id}
              title={saved.label || targetSerial}
              subtitle={nickname ? `"${nickname}"` : targetSerial}
              telemetryLabel={telemetryLabel}
              status={status}
              reconnecting={Boolean(
                reconnectingMap[savedHardwareKey(saved)] || reconnectingMap[saved.id],
              )}
              offlineTone="muted"
              isSelected={isSelected}
              isOfflineDimmed={true}
              transport={transport}
              battery={
                serial
                  ? resolveBatteryChipData(overviewBySerial[serial], batteryMap[serial])
                  : null
              }
              repairMsg={rowRepairMsg}
              isEditing={editingSerial === targetSerial}
              editingName={editingName}
              serial={serial}
              hasSession={Boolean(session)}
              isBusy={isBusy}
              showDoctor={
                failedSerial === saved.id ||
                Boolean(serial && failedSerial === serial) ||
                Boolean(device && device.state !== "device")
              }
              showDisconnect={Boolean(device?.transports?.wifi || (serial && isWifiSerial(serial)))}
              canQuickAction={Boolean(status !== "offline" && serial)}
              canForget={true}
              quickAction={quickAction}
              quickInput={quickInput}
              quickNote={serial && quickNote?.serial === serial ? quickNote.text : null}
              onSelect={() => onSelectDevice(targetSerial)}
              onStartEdit={() => handleStartNicknameEdit(targetSerial, nickname || "")}
              onEditingNameChange={setEditingName}
              onSaveEdit={() => void handleSaveNickname(nicknameKey, serial)}
              onCancelEdit={() => setEditingSerial(null)}
              onOpenPair={onOpenPair}
              onConnect={() => void handleConnect(saved)}
              onStop={() => session && void handleStop(session.id, targetSerial)}
              onLaunch={() => serial && void handleMirror(serial, saved)}
              onDoctor={() => openDoctor(targetSerial)}
              onDisconnect={() =>
                serial && void handleDisconnect(device?.transports?.wifi ?? serial)
              }
              onOpenQuickInput={(kind) => serial && void openQuickInput(serial, kind)}
              onQuickInputChange={setQuickInput}
              onSubmitQuickInput={() => serial && void submitQuickInput(serial)}
              onCloseQuickInput={() => setQuickAction(null)}
              onForget={() => void handleForget(saved.id)}
            />
          );
        })}

        {onOpenPair && <PairingEntryCard onOpenPair={onOpenPair} />}
      </div>
    </div>
  );
}
