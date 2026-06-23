import { useEffect, useState } from "react";
import { Loader2, RefreshCw, X } from "lucide-react";
import { QRCodeSVG } from "qrcode.react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { selectClass } from "@/lib/styles";
import { useAppStore } from "@/store/useAppStore";
import { connectDevice, discoverWireless, listDevices, pairDevice } from "@/lib/tauri";
import { upsertSaved } from "@/lib/savedDevices";
import { generatePairingChallenge, type PairingChallenge } from "@/lib/qr";
import type { MdnsService, SavedDevice } from "@/lib/types";

type Mode = "qr" | "mdns" | "manual";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const POLL_MS = 800;

export function PairDialog({ onClose }: { onClose: () => void }) {
  const setDevices = useAppStore((s) => s.setDevices);
  const setSavedDevices = useAppStore((s) => s.setSavedDevices);
  const setError = useAppStore((s) => s.setError);

  const [mode, setMode] = useState<Mode>("qr");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  // QR mode
  const [challenge, setChallenge] = useState<PairingChallenge | null>(() =>
    generatePairingChallenge(),
  );
  const [qrStatus, setQrStatus] = useState<string | null>(null);
  const [qrBusy, setQrBusy] = useState(false);

  // mDNS mode
  const [services, setServices] = useState<MdnsService[]>([]);
  const [selected, setSelected] = useState(""); // "host:port" of the pairing endpoint
  const [mdnsCode, setMdnsCode] = useState("");

  // Manual mode
  const [ip, setIp] = useState("");
  const [pairPort, setPairPort] = useState("");
  const [pairCode, setPairCode] = useState("");
  const [connectPort, setConnectPort] = useState("");

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

  // QR pairing state machine: poll for the pairing service the phone advertises
  // after scanning, pair with the embedded password, then discover + connect.
  useEffect(() => {
    if (mode !== "qr" || !challenge) return;
    let cancelled = false;
    (async () => {
      setError(null);
      setQrBusy(true);
      setQrStatus("Waiting for the phone to scan the code…");
      const scanDeadline = Date.now() + 120_000;
      let pairing: MdnsService | undefined;
      let connect: MdnsService | undefined;
      while (!cancelled && Date.now() < scanDeadline) {
        const found = await discoverWireless().catch(() => []);
        if (cancelled) return;
        const pairings = found.filter((s) => s.serviceType.includes("pairing"));
        pairing =
          pairings.find((s) => s.name === challenge.name) ??
          (pairings.length === 1 ? pairings[0] : undefined);
        if (pairing) {
          // Grab the connect endpoint from the same round if it's already up,
          // so we usually skip the separate connect-wait below.
          connect = found.find(
            (s) => s.host === pairing!.host && s.serviceType.includes("connect"),
          );
          break;
        }
        await sleep(POLL_MS);
      }
      if (cancelled) return;
      if (!pairing) {
        setQrBusy(false);
        setQrStatus(null);
        setError(
          "Timed out waiting for a scan. Keep the phone on the same Wi-Fi, or try Manual.",
        );
        return;
      }

      setQrStatus("Scanned ✓ — pairing…");
      try {
        await pairDevice(pairing.host, pairing.port, challenge.password);
      } catch (e) {
        setQrBusy(false);
        setQrStatus(null);
        setError(`Pairing failed: ${String(e)}`);
        return;
      }
      if (cancelled) return;

      setQrStatus("Paired ✓ — connecting (this can take a few seconds)…");
      const host = pairing.host;
      const connectDeadline = Date.now() + 30_000;
      while (!cancelled && !connect && Date.now() < connectDeadline) {
        const found = await discoverWireless().catch(() => []);
        if (cancelled) return;
        connect = found.find((s) => s.host === host && s.serviceType.includes("connect"));
        if (connect) break;
        await sleep(POLL_MS);
      }
      if (cancelled) return;
      if (!connect) {
        setQrBusy(false);
        setQrStatus(null);
        setError("Paired, but no connect service appeared. Use Auto-discover/Manual to finish.");
        return;
      }
      try {
        await connectDevice(connect.host, connect.port);
      } catch (e) {
        setQrBusy(false);
        setQrStatus(null);
        setError(`Connect failed: ${String(e)}`);
        return;
      }
      if (cancelled) return;
      await finishConnected(connect.host, connect.port);
    })();
    return () => {
      cancelled = true;
    };
  }, [mode, challenge]);

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

  function switchMode(next: Mode) {
    setError(null);
    setStatus(null);
    setQrStatus(null);
    setQrBusy(false);
    setMode(next);
    setChallenge(next === "qr" ? generatePairingChallenge() : null);
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
            <p className="text-xs text-zinc-500">
              On your phone: Settings → Developer options → <b>Wireless debugging</b> →{" "}
              <b>Pair device with QR code</b>, then scan this:
            </p>
            {challenge && (
              <div className="flex justify-center rounded-md bg-white p-4">
                <QRCodeSVG
                  value={challenge.payload}
                  size={208}
                  fgColor="#000000"
                  bgColor="#ffffff"
                />
              </div>
            )}
            <div className="flex items-center justify-between">
              <p className="flex items-center gap-2 text-xs text-zinc-500">
                {qrBusy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                {qrStatus ?? "Ready to scan."}
              </p>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  setError(null);
                  setChallenge(generatePairingChallenge());
                }}
              >
                <RefreshCw className="h-4 w-4" />
                New code
              </Button>
            </div>
          </div>
        )}

        {mode === "mdns" && (
          <div className="space-y-2">
            <p className="text-xs text-zinc-500">
              On your phone: <b>Wireless debugging</b> → <b>Pair device with pairing code</b>,
              then Refresh.
            </p>
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
            <p className="text-xs text-zinc-500">
              On your phone: <b>Wireless debugging</b> → <b>Pair device with pairing code</b>.
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
