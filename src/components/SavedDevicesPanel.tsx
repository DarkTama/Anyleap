import { useState } from "react";
import { Wifi, X } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/store/useAppStore";
import { connectDevice, listDevices } from "@/lib/tauri";
import { forgetSaved } from "@/lib/savedDevices";

export function SavedDevicesPanel() {
  const savedDevices = useAppStore((s) => s.savedDevices);
  const devices = useAppStore((s) => s.devices);
  const setDevices = useAppStore((s) => s.setDevices);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);
  const [busyId, setBusyId] = useState<string | null>(null);

  async function reconnect(host: string, port: number, id: string) {
    setBusyId(id);
    setError(null);
    try {
      await connectDevice(host, port);
      setDevices(await listDevices());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  async function forget(id: string) {
    setBusyId(id);
    try {
      setSavedDevices(await forgetSaved(id));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyId(null);
    }
  }

  if (savedDevices.length === 0) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Saved devices</CardTitle>
      </CardHeader>
      <CardContent>
        <ul className="space-y-2">
          {savedDevices.map((d) => {
            const connected = devices.some((dev) => dev.serial === d.lastSerial);
            return (
              <li
                key={d.id}
                className="flex items-center justify-between rounded-md border border-zinc-100 px-3 py-2 text-sm dark:border-zinc-800/60"
              >
                <span className="flex items-center gap-2">
                  <Wifi className="h-4 w-4 text-sky-500" />
                  <span className="font-mono text-xs">
                    {d.host}:{d.port}
                  </span>
                  {connected && (
                    <span className="text-xs text-green-600 dark:text-green-400">ready</span>
                  )}
                </span>
                <span className="flex gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => reconnect(d.host, d.port, d.id)}
                    disabled={busyId === d.id || connected}
                  >
                    {busyId === d.id ? "…" : "Reconnect"}
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    onClick={() => forget(d.id)}
                    disabled={busyId === d.id}
                    aria-label="Forget"
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </span>
              </li>
            );
          })}
        </ul>
      </CardContent>
    </Card>
  );
}
