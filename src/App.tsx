import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { StudioDeck } from "@/components/deck/StudioDeck";
import { PhoneView, type PhoneSubTab } from "@/components/phone/PhoneView";
import { ComputersView } from "@/components/computers/ComputersView";
import { SettingsPanel } from "@/components/SettingsPanel";
import { AppHeader, type ShellTab } from "@/components/shell/AppHeader";
import { CommandPalette } from "@/components/shell/CommandPalette";
import { ConnectionDoctor } from "@/components/ConnectionDoctor";
import { TimelineDrawer } from "@/components/deck/TimelinePanel";
import { PairDialog } from "@/components/PairDialog";
import { useAppStore } from "@/store/useAppStore";
import {
  getDeviceNotifications,
  listSessions,
  onSessionExited,
  onSessionReplaced,
  onSessionRestarting,
  onSessionStarted,
  startMirror,
  stopMirror,
  takeScreenshot,
  startRecording,
  stopRecording,
  openUrlOnDevice,
  getPhoneToggles,
  setPhoneToggle,
  setScrollFeel,
} from "@/lib/tauri";
import { MSG_NEEDS_REPAIR, resolveDeviceNickname, resolveDeviceTarget } from "@/lib/deviceStatus";
import { surfaceAdbError } from "@/lib/adbError";
import { refreshAndReconnectDevices } from "@/lib/deviceSync";
import { deviceHealth, reportDeviceError } from "@/lib/deviceHealth";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { listSaved } from "@/lib/savedDevices";
import { loadControlConfig } from "@/lib/controlConfig";
import { getNicknames, loadAppPrefs, loadModeDefaults, runFileSyncMigration } from "@/lib/persist";
import { resolveLaunchSettings, toScrollConfig, type MirrorMode } from "@/lib/modeSettings";
import { launchMode } from "@/lib/modeActions";
import {
  mirrorLabelForSerial,
  shouldCloseMirrorWindowOnExit,
  shouldKeepMirrorWindowOpen,
} from "@/lib/mirrorWindowPlan";
import {
  onPhoneNotifications,
  phoneHubSetSettings,
  phoneHubUnwatch,
  phoneHubWatch,
} from "@/services/phoneHubService";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import type { PhoneHubNotification } from "@/types/phone";
import { UpdateModal } from "@/components/shell/UpdateModal";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import {
  TRANSFER_BAR_CLOSE_WAIT_MS,
  TRANSFER_BAR_REVEAL_VERIFY_MS,
  decideTransferBarWindowAction,
} from "@/components/desktop/transferBarWindowPolicy";
import { loadAllowControl, useKvmStore } from "@/stores/kvmStore";
import { kvmDebugNote, kvmOpenUrlOnLinked, kvmSetDebugLog } from "@/services/kvmService";
import { logActivity, useActivityStore } from "@/stores/activityStore";
import { buildCommands, normalizeOpenableUrl } from "@/lib/commandPalette";
import type { StudioMode } from "@/components/deck/LaunchMatrix";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";

const EMPTY_NOTIFICATIONS: PhoneHubNotification[] = [];


function App() {
  const error = useAppStore((s) => s.error);
  const sessions = useAppStore((s) => s.sessions);
  const setSessions = useAppStore((s) => s.setSessions);
  const upsertSession = useAppStore((s) => s.upsertSession);
  const removeSession = useAppStore((s) => s.removeSession);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setControlConfig = useAppStore((s) => s.setControlConfig);
  const setModeDefaults = useAppStore((s) => s.setModeDefaults);
  const setAppPrefs = useAppStore((s) => s.setAppPrefs);
  const setError = useAppStore((s) => s.setError);
  const setNicknames = useAppStore((s) => s.setNicknames);
  const [errorDetails, setErrorDetails] = useState<string | null>(null);
  const [showDetails, setShowDetails] = useState(false);
  const [activeTab, setActiveTab] = useState<ShellTab>("devices");
  const [phoneSubTab, setPhoneSubTab] = useState<PhoneSubTab>("overview");
  const [selectedPhoneSerial, setSelectedPhoneSerial] = useState<string | null>(null);
  const [notificationsLoading, setNotificationsLoading] = useState(false);
  const [notificationsError, setNotificationsError] = useState<string | null>(null);
  const seenNotifKeysBySerialRef = useRef<Map<string, Set<string>>>(new Map());
  const prevOnlineSerialsRef = useRef<Set<string>>(new Set());
  const [pairOpen, setPairOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [hudNotice, setHudNotice] = useState<string | null>(null);
  const hudNoticeRef = useRef<string | null>(null);
  const hudNoticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const wasHudNoticeRef = useRef(false);
  const hudOpIdRef = useRef(0);
  const restartingSerialsRef = useRef<Set<string>>(new Set());
  const restartingModesRef = useRef<Map<string, MirrorMode>>(new Map());

  const devices = useAppStore((s) => s.devices);
  const selectedSerial = useAppStore((s) => s.selectedSerial);
  const openDoctor = useAppStore((s) => s.openDoctor);
  const setTimelineOpen = useAppStore((s) => s.setTimelineOpen);
  const kvmRunning = useKvmStore(
    (s) => s.status.allow_control ?? s.status.is_host_running
  );

  const desktopNotifications = useAppStore((s) => s.appPrefs.desktopNotifications);
  const onlineDevices = devices.filter((d) => d.state === "device");
  const activePhoneSerial =
    selectedPhoneSerial && onlineDevices.some((d) => d.serial === selectedPhoneSerial)
      ? selectedPhoneSerial
      : onlineDevices[0]?.serial ?? null;

  const phoneNotifications = usePhoneHubStore((s) =>
    activePhoneSerial
      ? s.notificationsBySerial[activePhoneSerial] ?? EMPTY_NOTIFICATIONS
      : EMPTY_NOTIFICATIONS,
  );
  const applyNotifications = usePhoneHubStore((s) => s.applyNotifications);
  const markSerialDisconnected = usePhoneHubStore((s) => s.markSerialDisconnected);
  const notificationPollSecs = usePhoneHubStore((s) => s.notificationPollSecs);

  const fetchPhoneNotifications = useCallback(async () => {
    if (!activePhoneSerial) return;
    setNotificationsLoading(true);
    setNotificationsError(null);
    try {
      let list;
      try {
        list = await getDeviceNotifications(activePhoneSerial);
      } catch (firstErr) {
        if (!(await reportDeviceError(firstErr, activePhoneSerial))) throw firstErr;
        list = await getDeviceNotifications(activePhoneSerial);
      }
      const normalized: PhoneHubNotification[] = list.map((n) => ({
        key: n.key,
        package: n.package,
        appLabel: n.app_label || n.package,
        title: n.title,
        text: n.text,
        postedUnix: n.posted_unix,
      }));
      applyNotifications(activePhoneSerial, normalized);
    } catch (e) {
      setNotificationsError(String(e));
    } finally {
      setNotificationsLoading(false);
    }
  }, [activePhoneSerial, applyNotifications]);

  // Clear per-serial memory-only state when a phone disconnects
  useEffect(() => {
    const current = new Set(onlineDevices.map((d) => d.serial));
    for (const prevSerial of prevOnlineSerialsRef.current) {
      if (!current.has(prevSerial)) {
        markSerialDisconnected(prevSerial);
        seenNotifKeysBySerialRef.current.delete(prevSerial);
      }
    }
    prevOnlineSerialsRef.current = current;
  }, [onlineDevices, markSerialDisconnected]);

  // Load persisted Phone Hub settings and push poll interval to backend
  useEffect(() => {
    void usePhoneHubStore.getState().loadPersistedSettings();
  }, []);

  useEffect(() => {
    void phoneHubSetSettings(notificationPollSecs).catch(() => {});
  }, [notificationPollSecs]);

  // Subscribe to backend phone-notifications events (replaces frontend 10s polling)
  useEffect(() => {
    const unlistenPromise = onPhoneNotifications(async ({ serial, items }) => {
      applyNotifications(serial, items);

      const seenMap = seenNotifKeysBySerialRef.current;
      const existingSeen = seenMap.get(serial);
      if (!existingSeen) {
        seenMap.set(serial, new Set(items.map((n) => n.key)));
        return;
      }

      const { desktopNotifications: toastsEnabled } = useAppStore.getState().appPrefs;
      const { mutedPackages } = usePhoneHubStore.getState();
      const mutedSet = new Set(mutedPackages);

      const fresh = items.filter((n) => !existingSeen.has(n.key));
      for (const n of fresh) {
        existingSeen.add(n.key);
      }

      if (!toastsEnabled) return;
      const unmutedFresh = fresh.filter((n) => !mutedSet.has(n.package));
      if (unmutedFresh.length === 0) return;

      let granted = await isPermissionGranted().catch(() => false);
      if (!granted) {
        const perm = await requestPermission().catch(() => "denied");
        granted = perm === "granted";
      }
      if (granted) {
        for (const n of unmutedFresh) {
          sendNotification({
            title: n.title || n.appLabel || n.package,
            body: n.text || undefined,
          });
        }
      }
    });

    return () => {
      unlistenPromise.then((un) => un()).catch(() => {});
    };
  }, [applyNotifications]);

  // Keep notification polling active in background when desktopNotifications is enabled and Phone tab is closed
  useEffect(() => {
    if (!activePhoneSerial || !desktopNotifications || activeTab === "phone") return;
    void phoneHubWatch(activePhoneSerial, ["notifications"]).catch(() => {});
    return () => {
      void phoneHubUnwatch(activePhoneSerial).catch(() => {});
    };
  }, [activePhoneSerial, desktopNotifications, activeTab]);

  // Event listeners + initial hydrate.
  useEffect(() => {
    const unlisteners = [
      onSessionRestarting((e) => {
        restartingSerialsRef.current.add(e.payload.serial);
        const prevMode = useAppStore.getState().sessionModes[e.payload.id];
        if (prevMode) {
          restartingModesRef.current.set(e.payload.serial, prevMode);
        }
      }),
      onSessionStarted((e) => {
        restartingSerialsRef.current.delete(e.payload.serial);
        const savedMode = restartingModesRef.current.get(e.payload.serial);
        restartingModesRef.current.delete(e.payload.serial);
        upsertSession(e.payload, savedMode);
        if (e.payload.mode !== "camera") {
          const st = useAppStore.getState();
          const sessionMode = st.sessionModes[e.payload.id] ?? savedMode ?? launchMode(st.currentMode);
          const modeKey = sessionMode === "camera" ? "mirror" : sessionMode;
          const scroll = st.modeDefaults[modeKey].scroll;
          void setScrollFeel(e.payload.serial, toScrollConfig(scroll)).catch(() => {});
          st.setDeviceToggle(e.payload.serial, "swipeScroll", scroll.style === "paged");
        }
      }),
      onSessionReplaced((e) => removeSession(e.payload)),
      onSessionExited((e) => {
        const state = useAppStore.getState();
        const endedSession = state.sessions.find((x) => x.id === e.payload.id);
        removeSession(e.payload.id);
        const targetSerial = e.payload.serial || endedSession?.serial;
        if (targetSerial) {
          restartingSerialsRef.current.delete(targetSerial);
          restartingModesRef.current.delete(targetSerial);
        }
        if (endedSession) {
          const resolved = resolveDeviceTarget(
            endedSession.serial,
            state.devices,
            state.savedDevices,
          );
          const dev = resolved.device ?? undefined;
          const saved = resolved.saved ?? undefined;
          const label =
            resolveDeviceNickname(state.nicknames, {
              serial: endedSession.serial,
              device: dev,
              saved,
            }) ||
            saved?.label ||
            dev?.model ||
            dev?.product ||
            endedSession.serial;
          const durationMs = Math.max(0, Date.now() - endedSession.started_at);
          logActivity({
            kind: endedSession.mode === "camera" ? "camera" : "mirror",
            title:
              endedSession.mode === "camera"
                ? `Camera Studio ${label}`
                : `Mirrored ${label}`,
            detail: e.payload.last_error || undefined,
            deviceId: resolved.hwSerial ?? undefined,
            durationMs,
          });
          if (endedSession.recording_path) {
            logActivity({
              kind: "mirror",
              title: `Recording saved (${label})`,
              detail: endedSession.recording_path,
              deviceId: resolved.hwSerial ?? undefined,
              durationMs,
            });
          }
        }
        if (targetSerial) {
          if (
            shouldCloseMirrorWindowOnExit({
              exitedSerial: targetSerial,
              activeSessions: useAppStore.getState().sessions,
              restartingSerials: restartingSerialsRef.current,
            })
          ) {
            useAppStore.getState().setDeviceToggle(targetSerial, "swipeScroll", false);
          }
          const mirrorLabel = mirrorLabelForSerial(targetSerial);
          const closeMirror = () => {
            if (
              !shouldCloseMirrorWindowOnExit({
                exitedSerial: targetSerial,
                activeSessions: useAppStore.getState().sessions,
                restartingSerials: restartingSerialsRef.current,
              })
            ) {
              return;
            }
            WebviewWindow.getByLabel(mirrorLabel)
              .then((w) => w?.destroy().catch(() => w?.close().catch(() => {})))
              .catch(() => {});
          };
          closeMirror();
          setTimeout(closeMirror, 150);
          setTimeout(closeMirror, 500);
        }
        if (e.payload.last_error) setError(e.payload.last_error);
        setErrorDetails(e.payload.stderr || null);
        setShowDetails(false);
      }),
      listen<{ reason?: string; proc?: string }>("kvm-leap-blocked", () => {
        const kvmState = useKvmStore.getState();
        const linkedName =
          kvmState.status.linked_peer?.name ||
          kvmState.topology.computers.find((c) => !c.is_local)?.name ||
          "Linked PC";
        const msg = `Can't move to ${linkedName} while an administrator window is focused`;
        setToast(msg);
        if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
        toastTimerRef.current = setTimeout(() => setToast(null), 4000);
        hudNoticeRef.current = msg;
        setHudNotice(msg);
        if (hudNoticeTimerRef.current) clearTimeout(hudNoticeTimerRef.current);
        hudNoticeTimerRef.current = setTimeout(() => {
          hudNoticeRef.current = null;
          setHudNotice(null);
        }, 4000);
      }),
    ];
    listSessions()
      .then(setSessions)
      .catch(() => {});
    return () => {
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
      if (hudNoticeTimerRef.current) clearTimeout(hudNoticeTimerRef.current);
      unlisteners.forEach((p) => p.then((un) => un()));
    };
  }, [setSessions, upsertSession, removeSession, setError]);

  // Load saved devices, migrate legacy IP:port entries, and auto-reconnect wireless phones.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const saved = await listSaved().catch(() => []);
      if (cancelled) return;
      setSavedDevices(saved);
      await refreshAndReconnectDevices({ silentRepair: true }).catch((e) => {
        if (!cancelled) surfaceAdbError(e, setError);
      });
    })();
    return () => {
      cancelled = true;
    };
  }, [setSavedDevices, setError]);

  // Background health check: notice dropped wireless phones and reconnect via mDNS.
  useEffect(() => {
    deviceHealth.start();
    return () => deviceHealth.stop();
  }, []);

  // Load the persisted control-bar config on launch.
  useEffect(() => {
    loadControlConfig().then(setControlConfig).catch(() => {});
  }, [setControlConfig]);

  // Load persisted quality settings + app prefs on launch; run the update check.
  useEffect(() => {
    loadModeDefaults()
      .then((m) => {
        setModeDefaults(m.modeDefaults, m.notice);
        const st = useAppStore.getState();
        for (const s of st.sessions) {
          if (s.mode === "camera") continue;
          const sessionMode = st.sessionModes[s.id] ?? launchMode(st.currentMode);
          const modeKey = sessionMode === "camera" ? "mirror" : sessionMode;
          const scroll = m.modeDefaults[modeKey].scroll;
          void setScrollFeel(s.serial, toScrollConfig(scroll)).catch(() => {});
          st.setDeviceToggle(s.serial, "swipeScroll", scroll.style === "paged");
        }
      })
      .catch(() => {});
    loadAppPrefs()
      .then((p) => {
        setAppPrefs(p);
        if (p.checkUpdates) void useAppUpdateStore.getState().checkForUpdates();
      })
      .catch(() => {});
    getNicknames().then(setNicknames).catch(() => {});
    void useActivityStore.getState().load();
    void useKvmStore.getState().setAllowControl(loadAllowControl());
    void kvmSetDebugLog(Boolean(useKvmStore.getState().settings.debugLog)).catch(() => {});
    useKvmStore.getState().syncContinuitySettings();
    void useKvmStore.getState().init();
    // SPEC-28 §5.4 step 6: keep file copy-paste off for users who had the retired gate off.
    void runFileSyncMigration({
      get: () => useKvmStore.getState().settings,
      update: (patch) => useKvmStore.getState().updateSettings(patch),
    }).catch(() => {});
  }, [setModeDefaults, setAppPrefs, setNicknames]);

  // Global Ctrl+K shortcut for Command Palette
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        const el = document.activeElement as HTMLElement | null;
        const tag = el?.tagName?.toLowerCase();
        if (tag === "input" || tag === "textarea" || tag === "select" || el?.isContentEditable) {
          return;
        }
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // Minimize to tray on close when enabled (read live pref to avoid a stale closure).
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested((e) => {
      if (useAppStore.getState().appPrefs.minimizeToTrayOnClose) {
        e.preventDefault();
        void getCurrentWindow().hide();
      }
    });
    return () => {
      unlisten.then((un) => un());
    };
  }, []);

  // Floating, always-on-top control window: open while mirroring, close when idle.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const allWindows = await WebviewWindow.getAll();
      if (cancelled) return;
      const existingControls = allWindows.find((w) => w.label === "controls");

      // Close any mirror windows whose sessions have ended (unless restarting in-place).
      for (const w of allWindows) {
        if (
          w.label.startsWith("mirror-") &&
          !shouldKeepMirrorWindowOpen(w.label, sessions, restartingSerialsRef.current)
        ) {
          await w.destroy().catch(() => w.close().catch(() => {}));
        }
      }
      if (cancelled) return;

      if (sessions.length > 0) {
        const lastSession = sessions[sessions.length - 1];
        const serial = lastSession.serial;
        const mode = lastSession.mode || "display";
        const mirrorLabel = mirrorLabelForSerial(serial);
        const existingMirror = await WebviewWindow.getByLabel(mirrorLabel);
        if (cancelled) return;
        const st = useAppStore.getState();
        const sessionMode = st.sessionModes[lastSession.id] ?? st.currentMode;
        const isEmbedded =
          sessionMode === "camera"
            ? st.modeDefaults.camera.embedded
            : resolveLaunchSettings(sessionMode, st.modeDefaults).embedded;
        const isFlex = sessionMode === "flex";

        const openMirrorWindow = (attempt = 0) => {
          if (cancelled) return;
          if (!useAppStore.getState().sessions.some((s) => s.serial === serial)) return;
          try {
            const w = new WebviewWindow(mirrorLabel, {
              url: `index.html?mirror=1&serial=${encodeURIComponent(serial)}&mode=${encodeURIComponent(mode)}&flex=${isFlex ? 1 : 0}`,
              title: mode === "camera" ? `AnyLeap Camera Studio — ${serial}` : `AnyLeap Workstation — ${serial}`,
              width: 480,
              height: 860,
              resizable: true,
              decorations: false,
              transparent: false,
            });
            w.once("tauri://error", (e) => {
              console.error("mirror window:", e);
              if (!cancelled && attempt < 3) {
                setTimeout(() => {
                  void WebviewWindow.getByLabel(mirrorLabel).then((live) => {
                    if (!live && !cancelled) openMirrorWindow(attempt + 1);
                  });
                }, 200);
              }
            });
          } catch (e) {
            console.error("mirror window create failed:", e);
          }
        };

        if (isEmbedded) {
          if (existingControls) {
            await existingControls.close().catch(() => {});
          }
          if (!existingMirror) {
            openMirrorWindow();
          } else if (!restartingSerialsRef.current.has(serial)) {
            // If an old wrapper was mid-teardown when the session reopened, recreate once it finishes destroying.
            void existingMirror
              .once("tauri://destroyed", () => {
                if (
                  !cancelled &&
                  !restartingSerialsRef.current.has(serial) &&
                  useAppStore.getState().sessions.some((s) => s.serial === serial)
                ) {
                  openMirrorWindow();
                }
              })
              .catch(() => {});
          }
        } else {
          if (existingMirror) {
            await existingMirror.close().catch(() => {});
          }
          if (!existingControls) {
            try {
              const w = new WebviewWindow("controls", {
                url: `index.html?control=1&serial=${encodeURIComponent(serial)}&flex=${isFlex ? 1 : 0}`,
                title: "AnyLeap Controls",
                width: 88,
                height: 560,
                x: 24,
                y: 80,
                resizable: false,
                decorations: false,
                alwaysOnTop: true,
                skipTaskbar: true,
                transparent: true,
                shadow: false,
              });
              w.once("tauri://error", (e) => console.error("controls window:", e));
            } catch (e) {
              console.error("controls window create failed:", e);
            }
          }
        }
      } else {
        if (existingControls) await existingControls.close().catch(() => {});
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sessions]);

  // Manage native OS topmost Desktop HUD Overlay window
  const kvmIndicatorHud = useKvmStore((s) => s.settings.indicatorHud);
  const kvmHudCorner = useKvmStore((s) => s.settings.indicatorHudCorner);
  const kvmHasPeers = useKvmStore((s) => s.status.connected_peers.length > 0 || s.topology.computers.length > 1);
  const continuityTransfers = useKvmStore((s) => s.continuityTransfers);
  const continuityNotice = useKvmStore((s) => s.continuityNotice);
  // The cursor is on this machine when it is a controller that is not trapped,
  // or a receiver that a remote controller has entered.
  const kvmOwnsCursor = useKvmStore(
    (s) => !!s.status.receiver_entered || (!!s.status.controller_edge && !s.status.is_input_trapped)
  );

  const destroyExistingHudWindow = useCallback(async () => {
    const w = await WebviewWindow.getByLabel("kvm-hud").catch(() => null);
    if (w) {
      await w.close().catch(() => {});
      void kvmDebugNote("HUD_CLOSE").catch(() => {});
    }
  }, []);

  const closeHudWindow = useCallback(async () => {
    hudOpIdRef.current += 1;
    await destroyExistingHudWindow();
  }, [destroyExistingHudWindow]);

  const openHudWindow = useCallback(
    async (opts: { width: number; height: number; url: string }): Promise<boolean> => {
      const opId = ++hudOpIdRef.current;
      const MARGIN = 16;
      const screenW = window.screen.availWidth || 1920;
      const screenH = window.screen.availHeight || 1080;
      const right = kvmHudCorner === "top-right" || kvmHudCorner === "bottom-right";
      const bottom = kvmHudCorner === "bottom-left" || kvmHudCorner === "bottom-right";
      const x = right ? Math.max(MARGIN, screenW - opts.width - MARGIN) : MARGIN;
      const y = bottom ? Math.max(MARGIN, screenH - opts.height - MARGIN) : MARGIN;

      await destroyExistingHudWindow();
      if (hudOpIdRef.current !== opId) {
        return false;
      }
      try {
        const w = new WebviewWindow("kvm-hud", {
          url: opts.url,
          title: "AnyLeap KVM HUD",
          width: opts.width,
          height: opts.height,
          x,
          y,
          resizable: false,
          decorations: false,
          alwaysOnTop: true,
          skipTaskbar: true,
          transparent: true,
          shadow: false,
          focus: false,
        });
        w.once("tauri://created", () => {
          void kvmDebugNote("HUD_CREATE").catch(() => {});
          void w.setIgnoreCursorEvents(true).catch(() => {});
        });
        w.once("tauri://error", (e) => console.error("kvm-hud window error:", e));
        return true;
      } catch (e) {
        console.error("Failed to spawn native desktop HUD overlay:", e);
        return false;
      }
    },
    [kvmHudCorner, destroyExistingHudWindow]
  );

  // Transient toast: appears on the machine that just received the cursor,
  // stays ~1.8 s, is click-through, and never lingers over the user's work.
  useEffect(() => {
    if (hudNoticeRef.current) return;
    let cancelled = false;
    let closeTimer: ReturnType<typeof setTimeout> | undefined;

    if (!(kvmIndicatorHud && kvmHasPeers && kvmOwnsCursor)) {
      void closeHudWindow();
      return;
    }

    (async () => {
      const opened = await openHudWindow({
        width: 200,
        height: 36,
        url: "index.html?kvm_hud=1",
      });
      if (cancelled || !opened) return;
      closeTimer = setTimeout(() => void closeHudWindow(), 1800);
    })();

    return () => {
      cancelled = true;
      if (closeTimer) clearTimeout(closeTimer);
    };
  }, [kvmIndicatorHud, kvmHudCorner, kvmHasPeers, kvmOwnsCursor, openHudWindow, closeHudWindow]);

  // Show admin-window leap-blocked notice in the floating HUD overlay even if
  // the main window is hidden behind the elevated window.
  useEffect(() => {
    if (hudNotice) {
      wasHudNoticeRef.current = true;
      void openHudWindow({
        width: 460,
        height: 44,
        url: `index.html?kvm_hud=1&notice=${encodeURIComponent(hudNotice)}`,
      });
    } else if (wasHudNoticeRef.current) {
      wasHudNoticeRef.current = false;
      void closeHudWindow();
    }
  }, [hudNotice, openHudWindow, closeHudWindow]);

  // Manage native OS topmost floating Transfer Bar window (independent of leap HUD)
  const transferBarQueueRef = useRef<Promise<void>>(Promise.resolve());
  const hadTransferBarContentRef = useRef(false);

  const spawnTransferBarWindow = useCallback(() => {
    const BAR_WIDTH = 340;
    const BAR_HEIGHT = 40;
    const MARGIN_BOTTOM = 24;
    const screenW = window.screen.availWidth || 1920;
    const screenH = window.screen.availHeight || 1080;
    const x = Math.max(0, Math.round((screenW - BAR_WIDTH) / 2));
    const y = Math.max(0, screenH - BAR_HEIGHT - MARGIN_BOTTOM);

    const state = useKvmStore.getState();
    const initPayload = JSON.stringify({
      transfers: state.continuityTransfers,
      notice: state.continuityNotice?.text ?? null,
    });

    try {
      const w = new WebviewWindow("kvm-transfer-bar", {
        url: `index.html?kvm_transfer_bar=1&init=${encodeURIComponent(initPayload)}`,
        title: "AnyLeap Continuity Transfers",
        width: BAR_WIDTH,
        height: BAR_HEIGHT,
        x,
        y,
        resizable: false,
        decorations: false,
        alwaysOnTop: true,
        skipTaskbar: true,
        transparent: true,
        shadow: false,
        focus: false,
      });
      void w.once("tauri://error", (e) => console.error("kvm-transfer-bar window error:", e));
    } catch (e) {
      console.error("Failed to spawn floating transfer bar:", e);
    }
  }, []);

  const closeTransferBarAndWait = useCallback(async () => {
    const w = await WebviewWindow.getByLabel("kvm-transfer-bar").catch(() => null);
    if (w) await w.close().catch(() => {});
    const deadline = Date.now() + TRANSFER_BAR_CLOSE_WAIT_MS;
    while (Date.now() < deadline) {
      const cur = await WebviewWindow.getByLabel("kvm-transfer-bar").catch(() => null);
      if (!cur) return;
      await new Promise((r) => setTimeout(r, 50));
    }
  }, []);

  // Bar window operations run serialized so an open never races a pending close.
  const syncTransferBarWindow = useCallback(
    async (hasContent: boolean) => {
      const hadContent = hadTransferBarContentRef.current;
      hadTransferBarContentRef.current = hasContent;
      const existing = await WebviewWindow.getByLabel("kvm-transfer-bar").catch(() => null);
      const action = decideTransferBarWindowAction({
        hasContent,
        hadContent,
        windowExists: existing !== null,
      });
      if (action === "close") {
        await closeTransferBarAndWait();
      } else if (action === "open") {
        spawnTransferBarWindow();
      } else if (action === "recreate") {
        await closeTransferBarAndWait();
        spawnTransferBarWindow();
      } else if (action === "reveal" && existing) {
        await existing.show().catch(() => {});
        // Window may be mid self-close: if it vanished, recreate it.
        await new Promise((r) => setTimeout(r, TRANSFER_BAR_REVEAL_VERIFY_MS));
        const still = await WebviewWindow.getByLabel("kvm-transfer-bar").catch(() => null);
        const st = useKvmStore.getState();
        if (!still && (st.continuityNotice !== null || st.continuityTransfers.length > 0)) {
          spawnTransferBarWindow();
        }
      }
    },
    [spawnTransferBarWindow, closeTransferBarAndWait],
  );

  const transferBarTriggerKey = `${continuityTransfers.map((t) => t.id).join(",")}|${continuityNotice?.id ?? ""}`;
  useEffect(() => {
    const hasContent = continuityTransfers.length > 0 || continuityNotice !== null;
    transferBarQueueRef.current = transferBarQueueRef.current
      .then(() => syncTransferBarWindow(hasContent))
      .catch(() => {});
  }, [
    transferBarTriggerKey,
    continuityTransfers.length,
    continuityNotice,
    syncTransferBarWindow,
  ]);

  const paletteCommands = useMemo(() => {
    const hasOnlineDevice = Boolean(
      selectedSerial &&
        devices.some((d) => d.serial === selectedSerial && d.state === "device"),
    );
    const activeSession = selectedSerial
      ? sessions.find((s) => s.serial === selectedSerial) ?? sessions[0]
      : sessions[0];
    const isRecording = Boolean(
      selectedSerial && sessions.some((s) => s.serial === selectedSerial && !!s.recording_path),
    );

    const togglePhone = (key: "wifi" | "bluetooth" | "dnd") => {
      if (!selectedSerial) return;
      void getPhoneToggles(selectedSerial)
        .then((t) => setPhoneToggle(selectedSerial, key, !t[key]))
        .catch((err) => setError(String(err)));
    };

    return buildCommands({
      selectedSerial,
      hasOnlineDevice,
      hasActiveSession: Boolean(activeSession),
      isRecording,
      kvmRunning,
      kvmHasPeers,
      switchTab: setActiveTab,
      startMode: (mode: StudioMode) => {
        const store = useAppStore.getState();
        store.setCurrentMode(mode);
        setActiveTab("devices");
        if (!selectedSerial) return;
        if (mode !== "camera") {
          void startMirror(selectedSerial, resolveLaunchSettings(mode, store.modeDefaults)).catch((err) => {
            const msg = String(err);
            store.setError(msg);
            logActivity({ kind: "mirror", title: "Failed to start mirror", detail: msg });
          });
        }
      },
      stopMirroring: () => {
        if (!activeSession) return;
        void stopMirror(activeSession.id).catch((err) => setError(String(err)));
      },
      toggleRecording: () => {
        if (!selectedSerial) return;
        const op = isRecording ? stopRecording(selectedSerial) : startRecording(selectedSerial);
        void op.then(upsertSession).catch((err) => setError(String(err)));
      },
      openLinkOnPhone: (urlArg?: string) => {
        if (!selectedSerial) return;
        void (async () => {
          let url = urlArg?.trim() || "";
          if (!url) {
            try {
              const clip = (await navigator.clipboard.readText()).trim();
              if (/^(https?:\/\/|geo:|mailto:)/i.test(clip)) url = clip;
            } catch {
              // clipboard unavailable
            }
          }
          if (!url) return;
          await openUrlOnDevice(selectedSerial, url);
          logActivity({ kind: "device", title: "Opened link on phone", detail: url });
        })().catch((err) => setError(String(err)));
      },
      togglePhoneWifi: () => togglePhone("wifi"),
      togglePhoneBluetooth: () => togglePhone("bluetooth"),
      togglePhoneDnd: () => togglePhone("dnd"),
      takeScreenshot: () => {
        if (!selectedSerial) return;
        void takeScreenshot(selectedSerial)
          .then((bytes) => {
            const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = `anyleap-${selectedSerial.replace(/[^a-zA-Z0-9_-]/g, "_")}-${Date.now()}.png`;
            a.click();
            URL.revokeObjectURL(url);
            logActivity({
              kind: "screenshot",
              title: `Screenshot saved`,
              detail: selectedSerial,
            });
          })
          .catch((err) => setError(String(err)));
      },
      openPair: () => setPairOpen(true),
      openDoctor: () => openDoctor(selectedSerial),
      openTimeline: () => setTimelineOpen(true),
      checkForUpdates: () => {
        void useAppUpdateStore.getState().checkForUpdates();
      },
      toggleKvmServer: () => {
        const kvm = useKvmStore.getState();
        const enabled = kvm.status.allow_control ?? kvm.status.is_host_running;
        void kvm.setAllowControl(!enabled);
      },
      openUrlOnLinkedPc: (urlArg?: string) => {
        void (async () => {
          const normalized = normalizeOpenableUrl(urlArg ?? "");
          if (!normalized) return;
          await kvmOpenUrlOnLinked(normalized);
          logActivity({ kind: "device", title: "Opened URL on Linked PC", detail: normalized });
        })().catch((err) => setError(String(err)));
      },
      toggleAudioBridge: () => {
        const kvm = useKvmStore.getState();
        const next = !kvm.settings.continuityAudioBridge;
        kvm.updateSettings({ continuityAudioBridge: next });
        setToast(next ? "Audio bridge enabled" : "Audio bridge disabled");
        if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
        toastTimerRef.current = setTimeout(() => setToast(null), 2500);
      },
    });
  }, [selectedSerial, devices, sessions, kvmRunning, kvmHasPeers, openDoctor, setTimelineOpen, setError]);

  return (
    <div className="relative flex h-screen w-screen flex-col overflow-hidden bg-ink-950 text-text">
      <AppHeader
        activeTab={activeTab}
        onTabChange={setActiveTab}
        onOpenPair={() => setPairOpen(true)}
        onOpenCommands={() => setPaletteOpen(true)}
      />

      {error && (
        <div className="popover-panel absolute top-16 left-1/2 -translate-x-1/2 z-50 w-11/12 max-w-xl rounded-lg border border-status-danger/40 bg-status-danger-bg px-3.5 py-2.5 text-xs text-status-danger shadow-elevated">
          <div className="flex items-start justify-between gap-2">
            <span className="font-mono">{error}</span>
            <div className="flex shrink-0 items-center gap-2">
              {error.includes(MSG_NEEDS_REPAIR) && (
                <button
                  type="button"
                  className="btn-tactile rounded bg-gold-400 px-2 py-0.5 text-[10px] font-bold uppercase text-ink-950 hover:bg-gold-600 cursor-pointer"
                  onClick={() => {
                    setError(null);
                    setErrorDetails(null);
                    setShowDetails(false);
                    setPairOpen(true);
                  }}
                >
                  Pair again
                </button>
              )}
              <button
                className="btn-tactile shrink-0 text-[10px] uppercase font-bold underline opacity-85 hover:opacity-100 cursor-pointer"
                onClick={() => {
                  setError(null);
                  setErrorDetails(null);
                  setShowDetails(false);
                }}
              >
                dismiss
              </button>
            </div>
          </div>
          {errorDetails && (
            <div className="mt-1">
              <button
                className="btn-tactile text-[10px] underline opacity-85 hover:opacity-100 cursor-pointer"
                onClick={() => setShowDetails((v) => !v)}
              >
                {showDetails ? "Hide technical log" : "Show technical log"}
              </button>
              {showDetails && (
                <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded bg-ink-950 p-2 font-mono text-[10px] text-status-danger">
                  {errorDetails}
                </pre>
              )}
            </div>
          )}
        </div>
      )}

      <main className="relative flex flex-1 overflow-hidden">
        {activeTab === "devices" && (
          <StudioDeck
            onOpenPair={() => setPairOpen(true)}
            onViewPhoneDetails={(serial) => {
              setSelectedPhoneSerial(serial);
              setPhoneSubTab("overview");
              setActiveTab("phone");
            }}
          />
        )}
        {activeTab === "phone" && (
          <PhoneView
            subTab={phoneSubTab}
            onSubTabChange={setPhoneSubTab}
            selectedSerial={activePhoneSerial}
            onSelectSerial={setSelectedPhoneSerial}
            notifications={phoneNotifications}
            notificationsLoading={notificationsLoading}
            notificationsError={notificationsError}
            onRefreshNotifications={() => void fetchPhoneNotifications()}
          />
        )}
        {activeTab === "computers" && <ComputersView />}
        {activeTab === "settings" && (
          <div className="flex flex-1 flex-col overflow-y-auto bg-ink-950 p-6 max-w-5xl mx-auto w-full">
            <SettingsPanel onOpenComputers={() => setActiveTab("computers")} />
          </div>
        )}
      </main>

      {pairOpen && <PairDialog onClose={() => setPairOpen(false)} />}
      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        commands={paletteCommands}
      />
      <ConnectionDoctor />
      <TimelineDrawer />
      <UpdateModal />
      {toast && (
        <div
          className="popover-panel pointer-events-none absolute bottom-4 left-1/2 -translate-x-1/2 z-50 whitespace-nowrap rounded-lg bg-ink-800 border border-gold-400/50 px-3 py-1.5 text-xs text-gold-400 shadow-elevated"
          style={{ "--transform-origin": "bottom center" } as React.CSSProperties}
        >
          {toast}
        </div>
      )}
    </div>
  );
}

export default App;
