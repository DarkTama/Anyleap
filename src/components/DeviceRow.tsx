import { useState } from "react";
import { Camera, Check, Gamepad2, Pencil, Smartphone, Square, Stethoscope, Unplug, Usb, Wifi, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DeviceStateBadge } from "./DeviceStateBadge";
import { ControlBar } from "./ControlBar";
import { useAppStore } from "@/store/useAppStore";
import { logActivity } from "@/stores/activityStore";
import { CameraLaunchDialog } from "./CameraLaunchDialog";
import {
  connectDevice,
  disconnectDevice,
  discoverWireless,
  listDevices,
  startMirror,
  stopMirror,
} from "@/lib/tauri";
import { forgetSaved } from "@/lib/savedDevices";
import {
  deviceStatusOf,
  type DeviceStatus,
  type SavedRow,
  resolveDeviceNickname,
  getNicknameKey,
} from "@/lib/deviceStatus";
import type { DeviceInfo } from "@/lib/types";
import { saveNickname } from "@/lib/settingsStore";

const STATUS_STYLE: Record<DeviceStatus, string> = {
  offline: "bg-ink-800 text-text-muted border border-ink-700",
  connected: "bg-status-success-bg text-status-success border border-status-success/40",
  mirroring: "bg-ink-800 text-gold-400 border border-gold-400/50",
};
const STATUS_LABEL: Record<DeviceStatus, string> = {
  offline: "Offline",
  connected: "Connected",
  mirroring: "Mirroring",
};

function StatusBadge({ status, mode }: { status: DeviceStatus; mode?: string }) {
  if (status === "mirroring" && mode === "camera") {
    return (
      <Badge className="bg-status-success-bg text-status-success border border-status-success/40">
        Camera Stream
      </Badge>
    );
  }
  return <Badge className={STATUS_STYLE[status]}>{STATUS_LABEL[status]}</Badge>;
}

function splitSerial(serial: string): [string, number] {
  const [host, port] = serial.split(":");
  return [host, Number(port)];
}

/** A connected device that is not a saved wireless device (USB or ad-hoc wireless). */
export function DeviceRow({ device }: { device: DeviceInfo }) {
  const settings = useAppStore((s) => s.settings);
  const sessions = useAppStore((s) => s.sessions);
  const setDevices = useAppStore((s) => s.setDevices);
  const setError = useAppStore((s) => s.setError);
  const controlConfig = useAppStore((s) => s.controlConfig);
  const nicknames = useAppStore((s) => s.nicknames);
  const setNickname = useAppStore((s) => s.setNickname);
  const openDoctor = useAppStore((s) => s.openDoctor);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [cameraOpen, setCameraOpen] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState("");

  const nickname = nicknames[device.serial];

  const handleSaveNickname = async () => {
    try {
      await saveNickname(device.serial, editName);
      setNickname(device.serial, editName);
      setEditing(false);
    } catch (e) {
      setError(String(e));
    }
  };

  const wireless = device.serial.includes(":");
  const status = deviceStatusOf(device, sessions);
  const session = sessions.find((s) => s.serial === device.serial);

  async function mirror() {
    setBusy(true);
    setError(null);
    try {
      await startMirror(device.serial, settings);
    } catch (e) {
      const msg = String(e);
      setError(msg);
      logActivity({ kind: "mirror", title: `Failed to mirror ${device.serial}`, detail: msg });
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    if (!session) return;
    setBusy(true);
    try {
      await stopMirror(session.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function disconnect() {
    const [host, port] = splitSerial(device.serial);
    if (!host || !port) return;
    setBusy(true);
    setError(null);
    try {
      await disconnectDevice(host, port);
      setDevices(await listDevices());
      logActivity({ kind: "device", title: `Disconnected ${device.serial}` });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="rounded-md border border-ink-700 bg-ink-900">
      <div className="flex items-center justify-between px-3 py-2 text-sm">
        <span className="flex items-center gap-2">
          {wireless ? (
            <Wifi className="h-4 w-4 text-mist-300" />
          ) : (
            <Usb className="h-4 w-4 text-text-muted" />
          )}
          {editing ? (
            <span className="flex items-center gap-1">
              <input
                type="text"
                value={editName}
                onChange={(e) => setEditName(e.target.value)}
                placeholder={device.model ?? "Device name"}
                className="h-6 w-28 rounded border border-ink-700 bg-ink-800 px-1.5 text-xs text-text-strong focus:outline-none focus:border-gold-400"
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleSaveNickname();
                  if (e.key === "Escape") setEditing(false);
                }}
                autoFocus
              />
              <Button size="icon" variant="ghost" className="h-5 w-5" onClick={handleSaveNickname} title="Save">
                <Check className="h-3 w-3 text-status-success" />
              </Button>
              <Button size="icon" variant="ghost" className="h-5 w-5" onClick={() => setEditing(false)} title="Cancel">
                <X className="h-3 w-3 text-text-muted" />
              </Button>
            </span>
          ) : (
            <span className="flex items-center gap-1.5">
              <span className="text-text-strong">{nickname ? nickname : (device.model ?? "—")}</span>
              {nickname && (
                <span className="text-xs text-text-muted font-normal">({device.model ?? "—"})</span>
              )}
              <Button
                size="icon"
                variant="ghost"
                className="h-5 w-5 p-0 text-text-muted hover:text-text-strong"
                onClick={() => {
                  setEditName(nickname ?? "");
                  setEditing(true);
                }}
                title="Rename device"
              >
                <Pencil className="h-3 w-3" />
              </Button>
            </span>
          )}
          <span className="font-mono text-xs text-text-muted">{device.serial}</span>
          {status === "offline" ? (
            <DeviceStateBadge state={device.state} />
          ) : (
            <StatusBadge status={status} mode={session?.mode} />
          )}
        </span>
        <span className="flex gap-2">
          {device.state !== "device" && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => openDoctor(device.serial)}
              title="Run Connection Doctor"
            >
              <Stethoscope className="h-3.5 w-3.5" />
              Troubleshoot
            </Button>
          )}
          {status === "mirroring" ? (
            <Button size="sm" variant="destructive" onClick={stop} disabled={busy}>
              <Square className="h-3.5 w-3.5" />
              Stop
            </Button>
          ) : (
            <Button
              size="sm"
              onClick={mirror}
              disabled={busy || device.state !== "device"}
            >
              <Smartphone className="h-4 w-4" />
              Mirror
            </Button>
          )}
          {status !== "mirroring" && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => setCameraOpen(true)}
              disabled={busy || device.state !== "device"}
              title="Stream Camera as Video / Webcam"
            >
              <Camera className="h-3.5 w-3.5" />
              Camera
            </Button>
          )}
          {wireless && (
            <Button size="sm" variant="outline" onClick={disconnect} disabled={busy}>
              <Unplug className="h-4 w-4" />
              Disconnect
            </Button>
          )}
          {status !== "offline" && (
            <Button
              size="icon"
              variant="ghost"
              onClick={() => setOpen((v) => !v)}
              aria-label="Controls"
            >
              <Gamepad2 className="h-4 w-4" />
            </Button>
          )}
        </span>
      </div>
      {open && status !== "offline" && (
        <ControlBar serial={device.serial} config={controlConfig} orientation="horizontal" sessionMode={session?.mode} />
      )}
      <CameraLaunchDialog
        isOpen={cameraOpen}
        onClose={() => setCameraOpen(false)}
        serial={device.serial}
        deviceName={device.model}
      />
    </div>
  );
}

/** A saved wireless device (may be offline, connected, or mirroring). */
export function SavedDeviceRow({ row }: { row: SavedRow }) {
  const { saved, device, status } = row;
  const settings = useAppStore((s) => s.settings);
  const sessions = useAppStore((s) => s.sessions);
  const setDevices = useAppStore((s) => s.setDevices);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);
  const controlConfig = useAppStore((s) => s.controlConfig);
  const nicknames = useAppStore((s) => s.nicknames);
  const setNickname = useAppStore((s) => s.setNickname);
  const openDoctor = useAppStore((s) => s.openDoctor);
  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState("");
  const serial = device?.serial;
  const nickname = resolveDeviceNickname(nicknames, { serial, saved });
  const nicknameKey = getNicknameKey({ serial, saved });
  const handleSaveNickname = async () => {
    try {
      await saveNickname(nicknameKey, editName);
      setNickname(nicknameKey, editName);
      if (serial && serial !== nicknameKey) {
        await saveNickname(serial, editName);
        setNickname(serial, editName);
      }
      setEditing(false);
    } catch (e) {
      setError(String(e));
    }
  };
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [cameraOpen, setCameraOpen] = useState(false);

  const session = serial ? sessions.find((s) => s.serial === serial) : undefined;

  async function reconnect() {
    setBusy(true);
    setError(null);
    try {
      const services = await discoverWireless().catch(() => []);
      const svc = services.find(
        (s) => s.host === saved.host && s.serviceType.includes("connect"),
      );
      await connectDevice(saved.host, svc?.port ?? saved.port);
      setDevices(await listDevices());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function mirror() {
    if (!serial) return;
    setBusy(true);
    setError(null);
    try {
      await startMirror(serial, settings);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    if (!session) return;
    setBusy(true);
    try {
      await stopMirror(session.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function disconnect() {
    if (!serial) return;
    const [host, port] = splitSerial(serial);
    setBusy(true);
    setError(null);
    try {
      await disconnectDevice(host, port);
      setDevices(await listDevices());
      logActivity({ kind: "device", title: `Disconnected ${device.serial}` });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function forget() {
    setBusy(true);
    try {
      setSavedDevices(await forgetSaved(saved.id));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="rounded-md border border-ink-700 bg-ink-900">
      <div className="flex items-center justify-between px-3 py-2 text-sm">
        <span className="flex items-center gap-2">
          <Wifi className="h-4 w-4 text-mist-300" />
          {editing ? (
            <span className="flex items-center gap-1">
              <input
                type="text"
                value={editName}
                onChange={(e) => setEditName(e.target.value)}
                placeholder={saved.label}
                className="h-6 w-28 rounded border border-ink-700 bg-ink-800 px-1.5 text-xs text-text-strong focus:outline-none focus:border-gold-400"
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleSaveNickname();
                  if (e.key === "Escape") setEditing(false);
                }}
                autoFocus
              />
              <Button size="icon" variant="ghost" className="h-5 w-5" onClick={handleSaveNickname} title="Save">
                <Check className="h-3 w-3 text-status-success" />
              </Button>
              <Button size="icon" variant="ghost" className="h-5 w-5" onClick={() => setEditing(false)} title="Cancel">
                <X className="h-3 w-3 text-text-muted" />
              </Button>
            </span>
          ) : (
            <span className="flex items-center gap-1.5">
              <span className="text-text-strong">{nickname ? nickname : saved.label}</span>
              {nickname && (
                <span className="text-xs text-text-muted font-normal">({saved.label})</span>
              )}
              <Button
                size="icon"
                variant="ghost"
                className="h-5 w-5 p-0 text-text-muted hover:text-text-strong"
                onClick={() => {
                  setEditName(nickname ?? "");
                  setEditing(true);
                }}
                title="Rename device"
              >
                <Pencil className="h-3 w-3" />
              </Button>
            </span>
          )}
          <span className="font-mono text-xs text-text-muted">
            {serial ?? `${saved.host}:${saved.port}`}
          </span>
          <StatusBadge status={status} mode={session?.mode} />
        </span>
        <span className="flex gap-2">
          {status === "offline" && (
            <>
              <Button size="sm" variant="outline" onClick={reconnect} disabled={busy}>
                {busy ? "…" : "Reconnect"}
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => openDoctor(serial ?? `${saved.host}:${saved.port}`)}
                title="Run Connection Doctor"
              >
                <Stethoscope className="h-3.5 w-3.5" />
                Troubleshoot
              </Button>
            </>
          )}
          {status === "mirroring" && (
            <Button size="sm" variant="destructive" onClick={stop} disabled={busy}>
              <Square className="h-3.5 w-3.5" />
              Stop
            </Button>
          )}
          {status === "connected" && (
            <Button size="sm" onClick={mirror} disabled={busy}>
              <Smartphone className="h-4 w-4" />
              Mirror
            </Button>
          )}
          {status === "connected" && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => setCameraOpen(true)}
              disabled={busy}
              title="Stream Camera as Video / Webcam"
            >
              <Camera className="h-3.5 w-3.5" />
              Camera
            </Button>
          )}
          {serial && status !== "offline" && (
            <Button size="sm" variant="outline" onClick={disconnect} disabled={busy}>
              <Unplug className="h-4 w-4" />
              Disconnect
            </Button>
          )}
          {serial && status !== "offline" && (
            <Button
              size="icon"
              variant="ghost"
              onClick={() => setOpen((v) => !v)}
              aria-label="Controls"
            >
              <Gamepad2 className="h-4 w-4" />
            </Button>
          )}
          <Button
            size="icon"
            variant="ghost"
            onClick={forget}
            disabled={busy}
            aria-label="Forget"
          >
            <X className="h-4 w-4" />
          </Button>
        </span>
      </div>
      {open && serial && status !== "offline" && (
        <ControlBar serial={serial} config={controlConfig} orientation="horizontal" sessionMode={session?.mode} />
      )}
      {serial && (
        <CameraLaunchDialog
          isOpen={cameraOpen}
          onClose={() => setCameraOpen(false)}
          serial={serial}
          deviceName={saved.label || device?.model || serial}
        />
      )}
    </div>
  );
}
