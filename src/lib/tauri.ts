import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CoreSettings,
  DeviceInfo,
  MdnsService,
  MirrorRect,
  SessionExited,
  SessionInfo,
} from "./types";

// --- Commands ---

export const listDevices = () => invoke<DeviceInfo[]>("list_devices");

export const startMirror = (serial: string, settings: CoreSettings) =>
  invoke<SessionInfo>("start_mirror", { serial, settings });

export const stopMirror = (sessionId: string) =>
  invoke<void>("stop_mirror", { sessionId });

export const listSessions = () => invoke<SessionInfo[]>("list_sessions");

// --- Events ---

export const onSessionStarted = (cb: EventCallback<SessionInfo>): Promise<UnlistenFn> =>
  listen<SessionInfo>("session-started", cb);

export const onSessionExited = (cb: EventCallback<SessionExited>): Promise<UnlistenFn> =>
  listen<SessionExited>("session-exited", cb);

// --- Wireless (M2) ---

export const discoverWireless = () => invoke<MdnsService[]>("discover_wireless");

export const pairDevice = (host: string, port: number, code: string) =>
  invoke<string>("pair_device", { host, port, code });

export const connectDevice = (host: string, port: number) =>
  invoke<string>("connect_device", { host, port });

export const disconnectDevice = (host: string, port: number) =>
  invoke<void>("disconnect_device", { host, port });

// --- Control (M4) ---

export const sendKeyevent = (serial: string, keycode: number) =>
  invoke<void>("send_keyevent", { serial, keycode });

export const openNotifications = (serial: string) =>
  invoke<void>("open_notifications", { serial });

export const restartWithScreenOff = (serial: string, off: boolean) =>
  invoke<SessionInfo>("restart_with_screen_off", { serial, off });

export const mirrorRect = (title: string) =>
  invoke<MirrorRect | null>("mirror_rect", { title });

export const toggleDeviceOrientation = (serial: string) =>
  invoke<string>("toggle_device_orientation", { serial });
