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
  connectDevice,
  discoverWireless,
  getDeviceNotifications,
  listDevices,
  listSessions,
  onSessionExited,
  onSessionStarted,
  startMirror,
  stopMirror,
  takeScreenshot,
  startRecording,
  stopRecording,
  openUrlOnDevice,
  getPhoneToggles,
  setPhoneToggle,
} from "@/lib/tauri";
import type { PhoneNotification } from "@/lib/types";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { listSaved } from "@/lib/savedDevices";
import { loadControlConfig } from "@/lib/controlConfig";
import { getNicknames, loadAppPrefs, loadQuality } from "@/lib/persist";
import { UpdateModal } from "@/components/shell/UpdateModal";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { loadAllowControl, useKvmStore } from "@/stores/kvmStore";
import { kvmDebugNote, kvmSetDebugLog } from "@/services/kvmService";
import { logActivity, useActivityStore } from "@/stores/activityStore";
import { buildCommands } from "@/lib/commandPalette";
import type { StudioMode } from "@/components/deck/LaunchMatrix";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";

function App() {
  const error = useAppStore((s) => s.error);
  const sessions = useAppStore((s) => s.sessions);
  const setDevices = useAppStore((s) => s.setDevices);
  const setSessions = useAppStore((s) => s.setSessions);
  const upsertSession = useAppStore((s) => s.upsertSession);
  const removeSession = useAppStore((s) => s.removeSession);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setControlConfig = useAppStore((s) => s.setControlConfig);
  const setSettings = useAppStore((s) => s.setSettings);
  const setPreset = useAppStore((s) => s.setPreset);
  const setAppPrefs = useAppStore((s) => s.setAppPrefs);
  const setError = useAppStore((s) => s.setError);
  const setNicknames = useAppStore((s) => s.setNicknames);
  const [errorDetails, setErrorDetails] = useState<string | null>(null);
  const [showDetails, setShowDetails] = useState(false);
  const [activeTab, setActiveTab] = useState<ShellTab>("devices");
  const [phoneSubTab, setPhoneSubTab] = useState<PhoneSubTab>("files");
  const [selectedPhoneSerial, setSelectedPhoneSerial] = useState<string | null>(null);
  const [phoneNotifications, setPhoneNotifications] = useState<PhoneNotification[]>([]);
  const [notificationsLoading, setNotificationsLoading] = useState(false);
  const [notificationsError, setNotificationsError] = useState<string | null>(null);
  const seenNotifKeysRef = useRef<Set<string>>(new Set());
  const seededNotifSerialRef = useRef<string | null>(null);
  const [pairOpen, setPairOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  const fetchPhoneNotifications = useCallback(async () => {
    if (!activePhoneSerial) return;
    setNotificationsLoading(true);
    setNotificationsError(null);
    try {
      const list = await getDeviceNotifications(activePhoneSerial);
      setPhoneNotifications(list);

      const isInitialSeed = seededNotifSerialRef.current !== activePhoneSerial;
      if (isInitialSeed) {
        seenNotifKeysRef.current = new Set(list.map((n) => n.key));
        seededNotifSerialRef.current = activePhoneSerial;
      } else if (desktopNotifications) {
        const fresh = list.filter((n) => !seenNotifKeysRef.current.has(n.key));
        for (const n of fresh) {
          seenNotifKeysRef.current.add(n.key);
        }
        if (fresh.length > 0) {
          let granted = await isPermissionGranted().catch(() => false);
          if (!granted) {
            const perm = await requestPermission().catch(() => "denied");
            granted = perm === "granted";
          }
          if (granted) {
            for (const n of fresh) {
              sendNotification({
                title: n.title || n.app_label || n.package,
                body: n.text || undefined,
              });
            }
          }
        }
      } else {
        for (const n of list) {
          seenNotifKeysRef.current.add(n.key);
        }
      }
    } catch (e) {
      setNotificationsError(String(e));
    } finally {
      setNotificationsLoading(false);
    }
  }, [activePhoneSerial, desktopNotifications]);

  const shouldPollNotifications =
    !!activePhoneSerial &&
    ((activeTab === "phone" && phoneSubTab === "notifications") || desktopNotifications);

  useEffect(() => {
    if (!shouldPollNotifications) return;
    void fetchPhoneNotifications();
    const id = setInterval(() => {
      void fetchPhoneNotifications();
    }, 10_000);
    return () => clearInterval(id);
  }, [shouldPollNotifications, fetchPhoneNotifications]);

  // Event listeners + initial hydrate.
  useEffect(() => {
    const unlisteners = [
      onSessionStarted((e) => upsertSession(e.payload)),
      onSessionExited((e) => {
        const state = useAppStore.getState();
        const endedSession = state.sessions.find((x) => x.id === e.payload.id);
        removeSession(e.payload.id);
        const targetSerial = e.payload.serial || endedSession?.serial;
        if (endedSession) {
          const dev = state.devices.find((d) => d.serial === endedSession.serial);
          const label =
            state.nicknames[endedSession.serial] ||
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
            durationMs,
          });
          if (endedSession.recording_path) {
            logActivity({
              kind: "mirror",
              title: `Recording saved (${label})`,
              detail: endedSession.recording_path,
              durationMs,
            });
          }
        }
        if (targetSerial) {
          const mirrorLabel = `mirror-${targetSerial.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
          const closeMirror = () => {
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
        setToast("Can't move to the Linked PC while an administrator window is focused");
        if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
        toastTimerRef.current = setTimeout(() => setToast(null), 4000);
      }),
    ];
    listDevices()
      .then(setDevices)
      .catch((e) => setError(String(e)));
    listSessions()
      .then(setSessions)
      .catch(() => {});
    return () => {
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
      unlisteners.forEach((p) => p.then((un) => un()));
    };
  }, [setDevices, setSessions, upsertSession, removeSession, setError]);

  // Load saved devices and best-effort auto-reconnect (offline phones are normal).
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const saved = await listSaved().catch(() => []);
      if (cancelled) return;
      setSavedDevices(saved);
      if (saved.length) {
        const services = await discoverWireless().catch(() => []);
        if (cancelled) return;
        await Promise.allSettled(
          saved.map((d) => {
            const svc = services.find(
              (s) => s.host === d.host && s.serviceType.includes("connect"),
            );
            return connectDevice(d.host, svc?.port ?? d.port);
          }),
        );
        if (cancelled) return;
        listDevices()
          .then(setDevices)
          .catch(() => {});
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [setSavedDevices, setDevices]);

  // Load the persisted control-bar config on launch.
  useEffect(() => {
    loadControlConfig().then(setControlConfig).catch(() => {});
  }, [setControlConfig]);

  // Load persisted quality settings + app prefs on launch; run the update check.
  useEffect(() => {
    loadQuality()
      .then((q) => {
        if (q) {
          setSettings(q.settings);
          setPreset(q.preset);
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
  }, [setSettings, setPreset, setAppPrefs, setNicknames]);

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
    (async () => {
      const allWindows = await WebviewWindow.getAll();
      const existingControls = allWindows.find((w) => w.label === "controls");
      const activeMirrorLabels = new Set(
        sessions.map((s) => `mirror-${s.serial.replace(/[^a-zA-Z0-9_-]/g, "_")}`)
      );

      // Close any mirror windows whose sessions have ended
      for (const w of allWindows) {
        if (w.label.startsWith("mirror-") && !activeMirrorLabels.has(w.label)) {
          await w.destroy().catch(() => w.close().catch(() => {}));
        }
      }

      if (sessions.length > 0) {
        const lastSession = sessions[sessions.length - 1];
        const serial = lastSession.serial;
        const mode = lastSession.mode || "display";
        const mirrorLabel = `mirror-${serial.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
        const existingMirror = await WebviewWindow.getByLabel(mirrorLabel);
        const isEmbedded = useAppStore.getState().settings.embedded;

        if (isEmbedded) {
          if (existingControls) {
            await existingControls.close().catch(() => {});
          }
          if (!existingMirror) {
            try {
              const w = new WebviewWindow(mirrorLabel, {
                url: `index.html?mirror=1&serial=${encodeURIComponent(serial)}&mode=${encodeURIComponent(mode)}`,
                title: mode === "camera" ? `AnyLeap Camera Studio — ${serial}` : `AnyLeap Workstation — ${serial}`,
                width: 480,
                height: 860,
                resizable: true,
                decorations: false,
                transparent: false,
              });
              w.once("tauri://error", (e) => console.error("mirror window:", e));
            } catch (e) {
              console.error("mirror window create failed:", e);
            }
          }
        } else {
          if (existingMirror) {
            await existingMirror.close().catch(() => {});
          }
          if (!existingControls) {
            try {
              const w = new WebviewWindow("controls", {
                url: `index.html?control=1&serial=${encodeURIComponent(serial)}`,
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
  }, [sessions]);

  // Manage native OS topmost Desktop HUD Overlay window
  const kvmIndicatorHud = useKvmStore((s) => s.settings.indicatorHud);
  const kvmHudCorner = useKvmStore((s) => s.settings.indicatorHudCorner);
  const kvmHasPeers = useKvmStore((s) => s.status.connected_peers.length > 0 || s.topology.computers.length > 1);
  // The cursor is on this machine when it is a controller that is not trapped,
  // or a receiver that a remote controller has entered.
  const kvmOwnsCursor = useKvmStore(
    (s) => !!s.status.receiver_entered || (!!s.status.controller_edge && !s.status.is_input_trapped)
  );

  // Transient toast: appears on the machine that just received the cursor,
  // stays ~1.8 s, is click-through, and never lingers over the user's work.
  useEffect(() => {
    const HUD_W = 200;
    const HUD_H = 36;
    const MARGIN = 16;
    let closeTimer: ReturnType<typeof setTimeout> | undefined;
    const closeHud = async () => {
      const w = await WebviewWindow.getByLabel("kvm-hud").catch(() => null);
      if (w) {
        await w.close().catch(() => {});
        void kvmDebugNote("HUD_CLOSE").catch(() => {});
      }
    };

    if (!(kvmIndicatorHud && kvmHasPeers && kvmOwnsCursor)) {
      void closeHud();
      return;
    }

    (async () => {
      const screenW = window.screen.availWidth || 1920;
      const screenH = window.screen.availHeight || 1080;
      const right = kvmHudCorner === "top-right" || kvmHudCorner === "bottom-right";
      const bottom = kvmHudCorner === "bottom-left" || kvmHudCorner === "bottom-right";
      const x = right ? Math.max(MARGIN, screenW - HUD_W - MARGIN) : MARGIN;
      const y = bottom ? Math.max(MARGIN, screenH - HUD_H - MARGIN) : MARGIN;

      await closeHud();
      try {
        const w = new WebviewWindow("kvm-hud", {
          url: "index.html?kvm_hud=1",
          title: "AnyLeap KVM HUD",
          width: HUD_W,
          height: HUD_H,
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
      } catch (e) {
        console.error("Failed to spawn native desktop HUD overlay:", e);
      }
      closeTimer = setTimeout(() => void closeHud(), 1800);
    })();

    return () => {
      if (closeTimer) clearTimeout(closeTimer);
    };
  }, [kvmIndicatorHud, kvmHudCorner, kvmHasPeers, kvmOwnsCursor]);

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
      switchTab: setActiveTab,
      startMode: (mode: StudioMode) => {
        const store = useAppStore.getState();
        store.setCurrentMode(mode);
        setActiveTab("devices");
        if (!selectedSerial) return;
        const base = store.settings;
        const nextSettings =
          mode === "flex"
            ? { ...base, flexDisplay: true, embedded: true, noWindowAspectRatioLock: true, renderFit: "unscaled" as const }
            : mode === "mirror"
              ? { ...base, flexDisplay: false, embedded: true, noWindowAspectRatioLock: false, renderFit: "letterbox" as const }
              : mode === "camera"
                ? { ...base, flexDisplay: false, embedded: true, noAudio: true, stayAwake: true }
                : { ...base, embedded: true, maxFps: 60, videoBitRate: 20_000_000, videoCodec: "av1" as const };
        store.setSettings(nextSettings);
        if (mode !== "camera") {
          void startMirror(selectedSerial, nextSettings).catch((err) => {
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
      openLinkOnPhone: () => {
        if (!selectedSerial) return;
        void (async () => {
          let initial = "";
          try {
            const clip = (await navigator.clipboard.readText()).trim();
            if (/^(https?:\/\/|geo:|mailto:)/i.test(clip)) initial = clip;
          } catch {
            // clipboard unavailable
          }
          const url = initial || window.prompt("Enter URL to open on phone:")?.trim() || "";
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
    });
  }, [selectedSerial, devices, sessions, kvmRunning, openDoctor, setTimelineOpen, setError]);

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
        {activeTab === "devices" && <StudioDeck onOpenPair={() => setPairOpen(true)} />}
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
          <div className="flex flex-1 flex-col overflow-y-auto bg-ink-950 p-6 max-w-4xl mx-auto w-full">
            <SettingsPanel />
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
