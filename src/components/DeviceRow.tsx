import { useState } from "react";
import { Gamepad2, Smartphone, Square, Unplug, Usb, Wifi, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DeviceStateBadge } from "./DeviceStateBadge";
import { ControlBar } from "./ControlBar";
import { useAppStore } from "@/store/useAppStore";
import {
  connectDevice,
  disconnectDevice,
  discoverWireless,
  listDevices,
  startMirror,
  stopMirror,
} from "@/lib/tauri";
import { forgetSaved } from "@/lib/savedDevices";
import { deviceStatusOf, type DeviceStatus, type SavedRow } from "@/lib/deviceStatus";
import type { DeviceInfo } from "@/lib/types";

const STATUS_STYLE: Record<DeviceStatus, string> = {
  offline: "bg-zinc-200 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300",
  connected: "bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300",
  mirroring: "bg-sky-100 text-sky-800 dark:bg-sky-900/40 dark:text-sky-300",
};
const STATUS_LABEL: Record<DeviceStatus, string> = {
  offline: "Offline",
  connected: "Connected",
  mirroring: "Mirroring",
};

function StatusBadge({ status }: { status: DeviceStatus }) {
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
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);

  const wireless = device.serial.includes(":");
  const status = deviceStatusOf(device, sessions);
  const session = sessions.find((s) => s.serial === device.serial);

  async function mirror() {
    setBusy(true);
    setError(null);
    try {
      await startMirror(device.serial, settings);
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
    const [host, port] = splitSerial(device.serial);
    if (!host || !port) return;
    setBusy(true);
    setError(null);
    try {
      await disconnectDevice(host, port);
      setDevices(await listDevices());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="rounded-md border border-zinc-100 dark:border-zinc-800/60">
      <div className="flex items-center justify-between px-3 py-2 text-sm">
        <span className="flex items-center gap-2">
          {wireless ? (
            <Wifi className="h-4 w-4 text-sky-500" />
          ) : (
            <Usb className="h-4 w-4 text-zinc-400" />
          )}
          <span>{device.model ?? "—"}</span>
          <span className="font-mono text-xs text-zinc-500">{device.serial}</span>
          {status === "offline" ? (
            <DeviceStateBadge state={device.state} />
          ) : (
            <StatusBadge status={status} />
          )}
        </span>
        <span className="flex gap-2">
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
        <ControlBar serial={device.serial} config={controlConfig} orientation="horizontal" onToggleOrientation={undefined} />
      )}
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
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);

  const serial = device?.serial;
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
    <div className="rounded-md border border-zinc-100 dark:border-zinc-800/60">
      <div className="flex items-center justify-between px-3 py-2 text-sm">
        <span className="flex items-center gap-2">
          <Wifi className="h-4 w-4 text-sky-500" />
          <span>{saved.label}</span>
          <span className="font-mono text-xs text-zinc-500">
            {serial ?? `${saved.host}:${saved.port}`}
          </span>
          <StatusBadge status={status} />
        </span>
        <span className="flex gap-2">
          {status === "offline" && (
            <Button size="sm" variant="outline" onClick={reconnect} disabled={busy}>
              {busy ? "…" : "Reconnect"}
            </Button>
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
        <ControlBar serial={serial} config={controlConfig} orientation="horizontal" onToggleOrientation={undefined} />
      )}
    </div>
  );
}
