import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CameraDeviceOption,
  CameraSettings,
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

export const setWheelSwipe = (serial: string, enabled: boolean) =>
  invoke<void>("set_wheel_swipe", { serial, enabled });

// --- Camera ---

export const listDeviceCameras = (serial: string) =>
  invoke<CameraDeviceOption[]>("list_device_cameras", { serial });

export const startCameraMirror = (serial: string, settings: CameraSettings) =>
  invoke<SessionInfo>("start_camera_mirror", { serial, settings });

export type CameraAction = "torch_on" | "torch_off" | "zoom_in" | "zoom_out";

export const sendCameraShortcut = (serial: string, action: CameraAction) =>
  invoke<void>("send_camera_shortcut", { serial, action });

// --- Embedded Mirror (SPEC-06) ---

export const embedMirror = (windowLabel: string, serial: string) =>
  invoke<void>("embed_mirror", { windowLabel, serial });

export const resizeEmbeddedMirror = (
  windowLabel: string,
  serial: string,
  width: number,
  height: number,
  x?: number,
  y?: number,
) =>
  invoke<void>("resize_embedded_mirror", {
    windowLabel,
    serial,
    width,
    height,
    x,
    y,
  });

// --- Escrcpy Features (SPEC-07) ---

export const handleDroppedFiles = (serial: string, paths: string[]) =>
  invoke<string>("handle_dropped_files", { serial, paths });

export const listInstalledApps = (serial: string) =>
  invoke<string[]>("list_installed_apps", { serial });

export const launchApp = (serial: string, packageName: string) =>
  invoke<void>("launch_app", { serial, packageName });

export const takeScreenshot = (serial: string) =>
  invoke<number[]>("take_screenshot", { serial });

export const getSystemResolution = () =>
  invoke<[number, number]>("get_system_resolution");

export const pushClipboardImage = (serial: string, imageBytes: number[], filename?: string) =>
  invoke<string>("push_clipboard_image", { serial, imageBytes, filename });

export const getBatteryInfo = (serial: string) =>
  invoke<{ level: number | null; charging: boolean }>("get_battery_info", { serial });
