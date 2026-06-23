import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";
import type { CoreSettings, DeviceInfo, SessionExited, SessionInfo } from "./types";

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
