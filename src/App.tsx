import { useEffect, useState } from "react";
import { StudioDeck } from "@/components/deck/StudioDeck";
import { useAppStore } from "@/store/useAppStore";
import {
  connectDevice,
  discoverWireless,
  listDevices,
  listSessions,
  onSessionExited,
  onSessionStarted,
} from "@/lib/tauri";
import { listSaved } from "@/lib/savedDevices";
import { loadControlConfig } from "@/lib/controlConfig";
import { getNicknames, loadAppPrefs, loadQuality } from "@/lib/persist";
import { checkForUpdates } from "@/lib/updateCheck";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";

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

  // Event listeners + initial hydrate.
  useEffect(() => {
    const unlisteners = [
      onSessionStarted((e) => upsertSession(e.payload)),
      onSessionExited((e) => {
        const exited = useAppStore
          .getState()
          .sessions.find((x) => x.id === e.payload.id);
        removeSession(e.payload.id);
        if (exited) {
          const mirrorLabel = `mirror-${exited.serial.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
          WebviewWindow.getByLabel(mirrorLabel)
            .then((w) => w?.close().catch(() => {}))
            .catch(() => {});
        }
        if (e.payload.last_error) setError(e.payload.last_error);
        setErrorDetails(e.payload.stderr || null);
        setShowDetails(false);
      }),
    ];
    listDevices()
      .then(setDevices)
      .catch((e) => setError(String(e)));
    listSessions()
      .then(setSessions)
      .catch(() => {});
    return () => {
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
        if (p.checkUpdates) void checkForUpdates();
      })
      .catch(() => {});
    getNicknames().then(setNicknames).catch(() => {});
  }, [setSettings, setPreset, setAppPrefs, setNicknames]);

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
          await w.close().catch(() => {});
        }
      }

      if (sessions.length > 0) {
        const lastSession = sessions[sessions.length - 1];
        const serial = lastSession.serial;
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
                url: `index.html?mirror=1&serial=${encodeURIComponent(serial)}`,
                title: `AnyLeap — ${serial}`,
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

  return (
    <div className="relative h-screen w-screen overflow-hidden bg-[#0a0c10] text-zinc-100">
      {error && (
        <div className="absolute top-14 left-1/2 -translate-x-1/2 z-50 w-11/12 max-w-xl rounded-lg border border-rose-500/40 bg-rose-950/90 px-3.5 py-2.5 text-xs text-rose-200 shadow-2xl backdrop-blur-md">
          <div className="flex items-start justify-between gap-2">
            <span className="font-mono">{error}</span>
            <button
              className="shrink-0 text-[10px] uppercase font-bold underline opacity-80 hover:opacity-100 cursor-pointer"
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
                className="text-[10px] underline opacity-80 hover:opacity-100 cursor-pointer"
                onClick={() => setShowDetails((v) => !v)}
              >
                {showDetails ? "Hide technical log" : "Show technical log"}
              </button>
              {showDetails && (
                <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded bg-black/50 p-2 font-mono text-[10px] text-rose-300">
                  {errorDetails}
                </pre>
              )}
            </div>
          )}
        </div>
      )}

      <StudioDeck />
    </div>
  );
}

export default App;
