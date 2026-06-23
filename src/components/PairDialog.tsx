import { useEffect, useState } from "react";
import { RefreshCw, X } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { selectClass } from "@/lib/styles";
import { useAppStore } from "@/store/useAppStore";
import { connectDevice, discoverWireless, listDevices, pairDevice } from "@/lib/tauri";
import { upsertSaved } from "@/lib/savedDevices";
import type { MdnsService, SavedDevice } from "@/lib/types";

type Mode = "mdns" | "manual";

export function PairDialog({ onClose }: { onClose: () => void }) {
  const setDevices = useAppStore((s) => s.setDevices);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);

  const [mode, setMode] = useState<Mode>("mdns");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  // mDNS mode
  const [services, setServices] = useState<MdnsService[]>([]);
  const [selected, setSelected] = useState(""); // "host:port" of the pairing endpoint
  const [mdnsCode, setMdnsCode] = useState("");

  // Manual mode
  const [ip, setIp] = useState("");
  const [pairPort, setPairPort] = useState("");
  const [pairCode, setPairCode] = useState("");
  const [connectPort, setConnectPort] = useState("");

  async function refreshServices() {
    setBusy(true);
    setError(null);
    try {
      setServices(await discoverWireless());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (mode === "mdns") void refreshServices();
  }, [mode]);

  async function finishConnected(host: string, port: number) {
    const device: SavedDevice = {
      id: host,
      label: host,
      host,
      port,
      lastSerial: `${host}:${port}`,
    };
    setSavedDevices(await upsertSaved(device));
    setDevices(await listDevices());
    onClose();
  }

  async function pairViaMdns() {
    const pairing = services.find((s) => `${s.host}:${s.port}` === selected);
    if (!pairing) {
      setError("Select a pairing endpoint first.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setStatus("Pairing…");
      await pairDevice(pairing.host, pairing.port, mdnsCode.trim());
      setStatus("Paired. Finding connect endpoint…");
      const fresh = await discoverWireless();
      const connect = fresh.find(
        (s) => s.host === pairing.host && s.serviceType.includes("connect"),
      );
      if (!connect) {
        setError(
          "Paired, but couldn't find the connect service. Switch to Manual and enter the IP:port shown on the Wireless debugging screen.",
        );
        return;
      }
      setStatus("Connecting…");
      await connectDevice(connect.host, connect.port);
      await finishConnected(connect.host, connect.port);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setStatus(null);
    }
  }

  async function pairViaManual() {
    const pPort = Number(pairPort);
    const cPort = Number(connectPort);
    if (!ip || !pPort || pairCode.trim().length !== 6) {
      setError("Enter the IP, pairing port, and 6-digit code.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setStatus("Pairing…");
      await pairDevice(ip, pPort, pairCode.trim());
      if (!cPort) {
        setError("Paired. Now enter the connect port and press Pair & connect again.");
        return;
      }
      setStatus("Connecting…");
      await connectDevice(ip, cPort);
      await finishConnected(ip, cPort);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setStatus(null);
    }
  }

  const pairingServices = services.filter((s) => s.serviceType.includes("pairing"));

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <CardTitle>Add wireless device</CardTitle>
        <Button size="icon" variant="ghost" onClick={onClose} aria-label="Close">
          <X className="h-4 w-4" />
        </Button>
      </CardHeader>
      <CardContent className="space-y-4">
        <p className="text-xs text-zinc-500">
          On your phone: Settings → Developer options → <b>Wireless debugging</b> →{" "}
          <b>Pair device with pairing code</b>. Keep that screen open while pairing.
        </p>

        <div className="flex gap-2">
          <Button
            size="sm"
            variant={mode === "mdns" ? "default" : "outline"}
            onClick={() => setMode("mdns")}
          >
            Auto-discover
          </Button>
          <Button
            size="sm"
            variant={mode === "manual" ? "default" : "outline"}
            onClick={() => setMode("manual")}
          >
            Manual
          </Button>
        </div>

        {mode === "mdns" ? (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-zinc-500">
                Discovered pairing endpoints
              </label>
              <Button size="sm" variant="outline" onClick={refreshServices} disabled={busy}>
                <RefreshCw className={busy ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                Refresh
              </Button>
            </div>
            {pairingServices.length === 0 ? (
              <p className="text-xs text-zinc-500">
                None found yet. Open "Pair device with pairing code" on the phone, then Refresh.
                If nothing appears, your network may block mDNS — use Manual.
              </p>
            ) : (
              <select
                className={selectClass}
                value={selected}
                onChange={(e) => setSelected(e.target.value)}
              >
                <option value="">Select a device…</option>
                {pairingServices.map((s) => (
                  <option key={`${s.host}:${s.port}`} value={`${s.host}:${s.port}`}>
                    {s.host}:{s.port}
                  </option>
                ))}
              </select>
            )}
            <input
              className={selectClass}
              placeholder="6-digit pairing code"
              inputMode="numeric"
              value={mdnsCode}
              onChange={(e) => setMdnsCode(e.target.value)}
            />
            <Button
              onClick={pairViaMdns}
              disabled={busy || !selected || mdnsCode.trim().length !== 6}
            >
              Pair &amp; connect
            </Button>
          </div>
        ) : (
          <div className="space-y-2">
            <input
              className={selectClass}
              placeholder="Phone IP (e.g. 192.168.1.42)"
              value={ip}
              onChange={(e) => setIp(e.target.value)}
            />
            <div className="grid grid-cols-2 gap-2">
              <input
                className={selectClass}
                placeholder="Pairing port"
                inputMode="numeric"
                value={pairPort}
                onChange={(e) => setPairPort(e.target.value)}
              />
              <input
                className={selectClass}
                placeholder="6-digit code"
                inputMode="numeric"
                value={pairCode}
                onChange={(e) => setPairCode(e.target.value)}
              />
            </div>
            <input
              className={selectClass}
              placeholder="Connect port (from Wireless debugging)"
              inputMode="numeric"
              value={connectPort}
              onChange={(e) => setConnectPort(e.target.value)}
            />
            <p className="text-[11px] text-zinc-400">
              Pairing port + code come from the "Pair with code" dialog; the connect port is on
              the main Wireless debugging screen.
            </p>
            <Button onClick={pairViaManual} disabled={busy}>
              Pair &amp; connect
            </Button>
          </div>
        )}

        {status && <p className="text-xs text-zinc-500">{status}</p>}
      </CardContent>
    </Card>
  );
}
