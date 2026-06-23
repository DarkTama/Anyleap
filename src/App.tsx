import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import { DevicesTab } from "@/components/DevicesTab";
import { SettingsPanel } from "@/components/SettingsPanel";
import { PairDialog } from "@/components/PairDialog";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
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
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

type Tab = "devices" | "settings";

function App() {
  const error = useAppStore((s) => s.error);
  const sessions = useAppStore((s) => s.sessions);
  const setDevices = useAppStore((s) => s.setDevices);
  const setSessions = useAppStore((s) => s.setSessions);
  const upsertSession = useAppStore((s) => s.upsertSession);
  const removeSession = useAppStore((s) => s.removeSession);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);
  const [pairOpen, setPairOpen] = useState(false);
  const [tab, setTab] = useState<Tab>("devices");

  // Event listeners + initial hydrate.
  useEffect(() => {
    const unlisteners = [
      onSessionStarted((e) => upsertSession(e.payload)),
      onSessionExited((e) => {
        removeSession(e.payload.id);
        if (e.payload.last_error) setError(e.payload.last_error);
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

  // Floating, always-on-top control window: open while mirroring, close when idle.
  useEffect(() => {
    (async () => {
      const existing = await WebviewWindow.getByLabel("controls");
      if (sessions.length > 0) {
        if (!existing) {
          const serial = sessions[sessions.length - 1].serial;
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
            });
            w.once("tauri://error", (e) => console.error("controls window:", e));
          } catch (e) {
            console.error("controls window create failed:", e);
          }
        }
      } else if (existing) {
        await existing.close();
      }
    })();
  }, [sessions]);

  return (
    <div className="min-h-screen bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-50">
      <header className="flex items-center justify-between border-b border-zinc-200 px-6 py-4 dark:border-zinc-800">
        <div>
          <h1 className="text-lg font-semibold">AnyLeap</h1>
          <p className="text-xs text-zinc-500">
            Effortless Android mirroring — USB &amp; wireless
          </p>
        </div>
        <Button
          size="sm"
          onClick={() => {
            setTab("devices");
            setPairOpen((v) => !v);
          }}
        >
          <Plus className="h-4 w-4" />
          Add wireless device
        </Button>
      </header>

      <nav className="flex gap-1 border-b border-zinc-200 px-6 dark:border-zinc-800">
        {(["devices", "settings"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={cn(
              "-mb-px border-b-2 px-4 py-2 text-sm font-medium transition-colors",
              tab === t
                ? "border-zinc-900 text-zinc-900 dark:border-zinc-50 dark:text-zinc-50"
                : "border-transparent text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200",
            )}
          >
            {t === "devices" ? "Devices" : "Settings"}
          </button>
        ))}
      </nav>

      {error && (
        <div className="mx-6 mt-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-900 dark:bg-red-950/40 dark:text-red-300">
          {error}
        </div>
      )}

      <main className="space-y-4 p-6">
        {tab === "devices" ? (
          <>
            {pairOpen && <PairDialog onClose={() => setPairOpen(false)} />}
            <DevicesTab />
          </>
        ) : (
          <SettingsPanel />
        )}
      </main>
    </div>
  );
}

export default App;
