import { useEffect, useState } from "react";
import { Plus } from "lucide-react";
import { DevicesPanel } from "@/components/DevicesPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { SessionsPanel } from "@/components/SessionsPanel";
import { SavedDevicesPanel } from "@/components/SavedDevicesPanel";
import { PairDialog } from "@/components/PairDialog";
import { Button } from "@/components/ui/button";
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

function App() {
  const error = useAppStore((s) => s.error);
  const setDevices = useAppStore((s) => s.setDevices);
  const setSessions = useAppStore((s) => s.setSessions);
  const upsertSession = useAppStore((s) => s.upsertSession);
  const removeSession = useAppStore((s) => s.removeSession);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);
  const [pairOpen, setPairOpen] = useState(false);

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
        // Wireless-debugging connect ports are dynamic; discover the current one
        // per saved host, falling back to the last-known port.
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

  return (
    <div className="min-h-screen bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-50">
      <header className="flex items-center justify-between border-b border-zinc-200 px-6 py-4 dark:border-zinc-800">
        <div>
          <h1 className="text-lg font-semibold">AnyLeap</h1>
          <p className="text-xs text-zinc-500">
            Effortless Android mirroring — USB &amp; wireless
          </p>
        </div>
        <Button size="sm" onClick={() => setPairOpen((v) => !v)}>
          <Plus className="h-4 w-4" />
          Add wireless device
        </Button>
      </header>

      {error && (
        <div className="mx-6 mt-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-900 dark:bg-red-950/40 dark:text-red-300">
          {error}
        </div>
      )}

      <main className="grid gap-4 p-6 lg:grid-cols-[1fr_360px]">
        <div className="space-y-4">
          {pairOpen && <PairDialog onClose={() => setPairOpen(false)} />}
          <DevicesPanel />
          <SavedDevicesPanel />
          <SessionsPanel />
        </div>
        <SettingsPanel />
      </main>
    </div>
  );
}

export default App;
