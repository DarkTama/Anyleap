import { useEffect } from "react";
import { DevicesPanel } from "@/components/DevicesPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { SessionsPanel } from "@/components/SessionsPanel";
import { useAppStore } from "@/store/useAppStore";
import {
  listDevices,
  listSessions,
  onSessionExited,
  onSessionStarted,
} from "@/lib/tauri";

function App() {
  const error = useAppStore((s) => s.error);
  const setDevices = useAppStore((s) => s.setDevices);
  const setSessions = useAppStore((s) => s.setSessions);
  const upsertSession = useAppStore((s) => s.upsertSession);
  const removeSession = useAppStore((s) => s.removeSession);
  const setError = useAppStore((s) => s.setError);

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

  return (
    <div className="min-h-screen bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-50">
      <header className="border-b border-zinc-200 px-6 py-4 dark:border-zinc-800">
        <h1 className="text-lg font-semibold">AnyLeap</h1>
        <p className="text-xs text-zinc-500">
          Effortless Android mirroring — USB (M1)
        </p>
      </header>

      {error && (
        <div className="mx-6 mt-4 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-900 dark:bg-red-950/40 dark:text-red-300">
          {error}
        </div>
      )}

      <main className="grid gap-4 p-6 lg:grid-cols-[1fr_360px]">
        <div className="space-y-4">
          <DevicesPanel />
          <SessionsPanel />
        </div>
        <SettingsPanel />
      </main>
    </div>
  );
}

export default App;
