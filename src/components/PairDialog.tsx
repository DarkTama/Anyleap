import { useEffect, useState } from "react";
import { Loader2, RefreshCw, X } from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { selectClass } from "@/lib/styles";
import { useAppStore } from "@/store/useAppStore";
import { logActivity } from "@/stores/activityStore";
import { connectDevice, discoverWireless, pairDevice } from "@/lib/tauri";
import { upsertSaved } from "@/lib/savedDevices";
import { resolveDeviceTarget } from "@/lib/deviceStatus";
import { syncDevicesAndMigrate } from "@/lib/deviceSync";
import { useQrPairingSession, type QrPaired } from "@/lib/qrPairingSession";
import type { MdnsService, SavedDevice } from "@/lib/types";

type Mode = "qr" | "mdns" | "manual";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const POLL_MS = 800;

export function PairDialog({ onClose }: { onClose: () => void }) {
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setSelectedSerial = useAppStore((s) => s.setSelectedSerial);
  const setError = useAppStore((s) => s.setError);

  const [mode, setMode] = useState<Mode>("qr");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  // QR mode
  const [qrConnectStatus, setQrStatus] = useState<string | null>(null);

  // mDNS mode
  const [services, setServices] = useState<MdnsService[]>([]);
  const [selected, setSelected] = useState(""); // "host:port" of the pairing endpoint
  const [mdnsCode, setMdnsCode] = useState("");

  // Manual mode
  const [ip, setIp] = useState("");
  const [pairPort, setPairPort] = useState("");
  const [pairCode, setPairCode] = useState("");
  const [connectPort, setConnectPort] = useState("");

  async function finishConnected(host: string, port: number, hwSerial?: string | null) {
    const hw = hwSerial || undefined;
    const device: SavedDevice = {
      id: hw ?? host,
      hw_serial: hw,
      label: host,
      host,
      port,
      lastSerial: `${host}:${port}`,
      transports: { wifi: `${host}:${port}` },
    };
    const nextSaved = await upsertSaved(device);
    setSavedDevices(nextSaved);
    const { devices } = await syncDevicesAndMigrate(undefined, undefined, { force: true });
    const resolved = resolveDeviceTarget(hw ?? `${host}:${port}`, devices, nextSaved);
    if (resolved.activeSerial) {
      setSelectedSerial(resolved.activeSerial);
    }
    onClose();
  }

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

  // After the session pairs, discover + connect the phone.
  async function connectAfterPair({ pairing, connect: early }: QrPaired) {
    try {
      setQrStatus("Paired ✓ — connecting (this can take a few seconds)…");
      let connect = early;
      const connectDeadline = Date.now() + 30_000;
      while (!connect && Date.now() < connectDeadline) {
        const found = await discoverWireless().catch(() => []);
        connect = found.find(
          (s) => s.host === pairing.host && s.serviceType.includes("connect"),
        );
        if (connect) break;
        await sleep(POLL_MS);
      }
      if (!connect) {
        setQrStatus(null);
        setError("Paired, but no connect service appeared. Use Auto-discover/Manual to finish.");
        return;
      }
      try {
        await connectDevice(connect.host, connect.port);
      } catch (e) {
        setQrStatus(null);
        setError(`Connect failed: ${String(e)}`);
        return;
      }
      await finishConnected(connect.host, connect.port, connect.hwSerial);
      logActivity({
        kind: "device",
        title: `Paired & connected ${connect.host}:${connect.port}`,
      });
    } catch (e) {
      setQrStatus(null);
      setError(String(e));
    }
  }

  const qr = useQrPairingSession(mode === "qr", {
    discover: () => discoverWireless().catch(() => []),
    pair: pairDevice,
    onPaired: (r) => void connectAfterPair(r),
  });
  const challenge = qr.challenge;
  const qrBusy = qr.phase === "pairing" || qr.phase === "paired";
  const qrStatus =
    qrConnectStatus ??
    (qr.phase === "listening"
      ? "Waiting for the phone to scan the code…"
      : qr.phase === "pairing"
        ? "Scanned ✓ — pairing…"
        : null);
  const mmss = `${Math.floor(qr.secondsLeft / 60)}:${String(qr.secondsLeft % 60).padStart(2, "0")}`;

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
      await finishConnected(connect.host, connect.port, connect.hwSerial);
      logActivity({
        kind: "device",
        title: `Paired & connected ${connect.host}:${connect.port}`,
      });
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
      logActivity({
        kind: "device",
        title: `Paired & connected ${ip}:${cPort}`,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setStatus(null);
    }
  }

  function switchMode(next: Mode) {
    setError(null);
    setStatus(null);
    setQrStatus(null);
    setMode(next);
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
        <div className="flex gap-2">
          <Button
            size="sm"
            variant={mode === "qr" ? "default" : "outline"}
            onClick={() => switchMode("qr")}
          >
            QR code
          </Button>
          <Button
            size="sm"
            variant={mode === "mdns" ? "default" : "outline"}
            onClick={() => switchMode("mdns")}
          >
            Auto-discover
          </Button>
          <Button
            size="sm"
            variant={mode === "manual" ? "default" : "outline"}
            onClick={() => switchMode("manual")}
          >
            Manual
          </Button>
        </div>

        {mode === "qr" && (
          <div className="space-y-3">
            <p className="text-xs text-text-muted">
              On your phone: Settings → Developer options → <b className="text-text-strong">Wireless debugging</b> →{" "}
              <b className="text-text-strong">Pair device with QR code</b>, then scan this:
            </p>
            <div className="relative flex justify-center rounded-md bg-white p-4">
              {challenge ? (
                <QRCodeSVG
                  value={challenge.payload}
                  size={208}
                  fgColor="#000000"
                  bgColor="#ffffff"
                />
              ) : (
                <div className="flex h-[208px] w-[208px] items-center justify-center text-center text-sm text-neutral-500">
                  {qr.phase === "expired"
                    ? "Expired"
                    : qr.phase === "failed"
                      ? "Pairing failed"
                      : "No active code"}
                </div>
              )}
            </div>
            {qr.phase === "listening" && (
              <p className="text-center text-xs text-text-muted">Code refreshes in {mmss}</p>
            )}
            {qr.phase === "failed" && qr.error && (
              <p className="text-xs text-red-400">Pairing failed: {qr.error}</p>
            )}
            <div className="flex items-center justify-between">
              <p className="flex items-center gap-2 text-xs text-text-muted">
                {qrBusy && <Loader2 className="h-3.5 w-3.5 animate-spin text-gold-400" />}
                {qrStatus ?? (qr.phase === "expired" ? "Code expired." : "")}
              </p>
              {(qr.phase === "expired" || qr.phase === "failed" || qr.phase === "listening") && (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setError(null);
                    setQrStatus(null);
                    qr.restart();
                  }}
                >
                  <RefreshCw className="h-4 w-4" />
                  {qr.phase === "expired"
                    ? "Expired – Refresh"
                    : qr.phase === "failed"
                      ? "Try again"
                      : "New code"}
                </Button>
              )}
            </div>
          </div>
        )}

        {mode === "mdns" && (
          <div className="space-y-2">
            <p className="text-xs text-text-muted">
              On your phone: <b className="text-text-strong">Wireless debugging</b> → <b className="text-text-strong">Pair device with pairing code</b>,
              then Refresh.
            </p>
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-text-muted">
                Discovered pairing endpoints
              </label>
              <Button size="sm" variant="outline" onClick={refreshServices} disabled={busy}>
                <RefreshCw className={busy ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                Refresh
              </Button>
            </div>
            {pairingServices.length === 0 ? (
              <p className="text-xs text-text-muted">
                None found yet. Open the pairing screen, then Refresh. If nothing appears,
                your network may block mDNS — use Manual.
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
        )}

        {mode === "manual" && (
          <div className="space-y-2">
            <p className="text-xs text-text-muted">
              On your phone: <b className="text-text-strong">Wireless debugging</b> → <b className="text-text-strong">Pair device with pairing code</b>.
            </p>
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
            <p className="text-[11px] text-text-muted">
              Pairing port + code come from the "Pair with code" dialog; the connect port is on
              the main Wireless debugging screen.
            </p>
            <Button onClick={pairViaManual} disabled={busy}>
              Pair &amp; connect
            </Button>
          </div>
        )}

        {status && <p className="text-xs text-text-muted">{status}</p>}
      </CardContent>
    </Card>
  );
}
