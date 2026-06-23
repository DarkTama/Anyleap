import { useState } from "react";
import { RefreshCw, Smartphone } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { DeviceStateBadge } from "./DeviceStateBadge";
import { useAppStore } from "@/store/useAppStore";
import { listDevices, startMirror } from "@/lib/tauri";

export function DevicesPanel() {
  const devices = useAppStore((s) => s.devices);
  const settings = useAppStore((s) => s.settings);
  const setDevices = useAppStore((s) => s.setDevices);
  const setError = useAppStore((s) => s.setError);
  const [refreshing, setRefreshing] = useState(false);
  const [launching, setLaunching] = useState<string | null>(null);

  async function refresh() {
    setRefreshing(true);
    setError(null);
    try {
      setDevices(await listDevices());
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  }

  async function mirror(serial: string) {
    setLaunching(serial);
    setError(null);
    try {
      await startMirror(serial, settings);
    } catch (e) {
      setError(String(e));
    } finally {
      setLaunching(null);
    }
  }

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>Devices</CardTitle>
        <Button size="sm" variant="outline" onClick={refresh} disabled={refreshing}>
          <RefreshCw className={refreshing ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
          Refresh
        </Button>
      </CardHeader>
      <CardContent>
        {devices.length === 0 ? (
          <p className="text-sm text-zinc-500">
            No devices found. Plug in a phone with USB debugging enabled, authorize the
            prompt on the phone, then click Refresh.
          </p>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-zinc-200 text-left text-xs text-zinc-500 dark:border-zinc-800">
                <th className="py-2 font-medium">Device</th>
                <th className="py-2 font-medium">Serial</th>
                <th className="py-2 font-medium">State</th>
                <th className="py-2" />
              </tr>
            </thead>
            <tbody>
              {devices.map((d) => (
                <tr
                  key={d.serial}
                  className="border-b border-zinc-100 dark:border-zinc-800/60"
                >
                  <td className="py-2">{d.model ?? "—"}</td>
                  <td className="py-2 font-mono text-xs">{d.serial}</td>
                  <td className="py-2">
                    <DeviceStateBadge state={d.state} />
                  </td>
                  <td className="py-2 text-right">
                    <Button
                      size="sm"
                      onClick={() => mirror(d.serial)}
                      disabled={d.state !== "device" || launching === d.serial}
                    >
                      <Smartphone className="h-4 w-4" />
                      {launching === d.serial ? "Starting…" : "Mirror"}
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </CardContent>
    </Card>
  );
}
