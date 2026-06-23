import { useState } from "react";
import { RefreshCw } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { DeviceRow, SavedDeviceRow } from "./DeviceRow";
import { useAppStore } from "@/store/useAppStore";
import { listDevices } from "@/lib/tauri";
import { buildRows } from "@/lib/deviceStatus";

export function DevicesTab() {
  const devices = useAppStore((s) => s.devices);
  const savedDevices = useAppStore((s) => s.savedDevices);
  const sessions = useAppStore((s) => s.sessions);
  const setDevices = useAppStore((s) => s.setDevices);
  const setError = useAppStore((s) => s.setError);
  const [refreshing, setRefreshing] = useState(false);

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

  const { savedRows, otherDevices } = buildRows(devices, savedDevices, sessions);

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>Devices</CardTitle>
        <Button size="sm" variant="outline" onClick={refresh} disabled={refreshing}>
          <RefreshCw className={refreshing ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
          Refresh
        </Button>
      </CardHeader>
      <CardContent className="space-y-4">
        {savedRows.length > 0 && (
          <section className="space-y-2">
            <h4 className="text-xs font-medium text-zinc-500">Saved</h4>
            {savedRows.map((r) => (
              <SavedDeviceRow key={r.saved.id} row={r} />
            ))}
          </section>
        )}

        <section className="space-y-2">
          <h4 className="text-xs font-medium text-zinc-500">Other devices</h4>
          {otherDevices.length === 0 ? (
            <p className="text-sm text-zinc-500">
              No other devices. Plug in a phone with USB debugging enabled, or add a
              wireless device.
            </p>
          ) : (
            otherDevices.map((d) => <DeviceRow key={d.serial} device={d} />)
          )}
        </section>
      </CardContent>
    </Card>
  );
}
