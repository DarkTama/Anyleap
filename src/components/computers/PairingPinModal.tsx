import { useEffect, useState } from "react";
import { X, KeyRound, Copy, Check, ShieldCheck, ArrowRight, Power } from "lucide-react";
import { useKvmStore } from "@/stores/kvmStore";
import { kvmPairPeer } from "@/services/kvmService";
import { usePresence } from "@/lib/usePresence";

export function PairingPinModal() {
  const isModalOpen = useKvmStore((s) => s.isPairingModalOpen);
  const closeModal = useKvmStore((s) => s.closePairingModal);
  const status = useKvmStore((s) => s.status);
  const selectedPeer = useKvmStore((s) => s.selectedPeer);
  const setAllowControl = useKvmStore((s) => s.setAllowControl);
  const forgetPeer = useKvmStore((s) => s.forgetPeer);
  const refreshStatus = useKvmStore((s) => s.refreshStatus);
  const isLoading = useKvmStore((s) => s.isLoading);
  const [copied, setCopied] = useState(false);
  const [targetPin, setTargetPin] = useState("");
  const [isVerifying, setIsVerifying] = useState(false);
  const [verifiedSuccess, setVerifiedSuccess] = useState(false);
  const { mounted, closing } = usePresence(isModalOpen, 140);

  // The pairing code rotates after repeated wrong attempts; keep it fresh while visible.
  const showOwnPin = isModalOpen && !selectedPeer;
  useEffect(() => {
    if (!showOwnPin) return;
    void refreshStatus();
    const t = setInterval(() => void refreshStatus(), 3000);
    return () => clearInterval(t);
  }, [showOwnPin, refreshStatus]);

  if (!mounted) return null;

  const allowControl = status.allow_control ?? status.is_host_running;
  const pin = allowControl ? status.security_pin : "";

  const handleCopy = async () => {
    if (!pin) return;
    try {
      await navigator.clipboard.writeText(pin);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard unavailable: the pairing code stays visible for manual entry.
    }
  };

  const handleVerifyRemote = async () => {
    if (!targetPin.trim() || !selectedPeer) return;
    setIsVerifying(true);
    try {
      await kvmPairPeer(selectedPeer.ip, selectedPeer.port, targetPin.trim());
      setVerifiedSuccess(true);
      setTimeout(() => {
        closeModal();
      }, 1200);
    } catch (e) {
      alert(`Pairing failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setIsVerifying(false);
    }
  };

  return (
    <div
      data-state={closing ? "closing" : "open"}
      className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4"
    >
      <div
        data-state={closing ? "closing" : "open"}
        className="modal-panel relative w-full max-w-md rounded-2xl border border-ink-700 bg-ink-900 shadow-elevated p-6 space-y-5"
      >
        <div className="flex items-center justify-between border-b border-ink-700 pb-3">
          <div className="flex items-center gap-2">
            <div className="grid h-8 w-8 place-items-center rounded-lg bg-ink-800 text-gold-400 border border-gold-400/40">
              <KeyRound className="h-4 w-4" />
            </div>
            <div>
              <h2 className="text-sm font-semibold text-text-strong">Pairing code</h2>
              <p className="text-[11px] text-text-muted">
                {selectedPeer
                  ? `Pairing with ${selectedPeer.name}`
                  : "One-time pairing code for first-time link"}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={closeModal}
            className="btn-tactile rounded-lg p-1 text-text-muted hover:bg-ink-800 hover:text-text-strong cursor-pointer"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* This Computer's PIN — only relevant when THIS PC is the one being controlled */}
        {showOwnPin && (
          <div className="flex flex-col items-center justify-center space-y-2 rounded-xl border border-ink-700 bg-ink-950 p-4 text-center">
            {pin ? (
              <>
                <span className="text-[11px] text-text-muted">
                  Enter this pairing code on the Main PC:
                </span>
                <div className="flex items-center gap-2 rounded-lg border border-gold-400/50 bg-ink-800 px-4 py-2">
                  <span className="font-mono text-xl font-black tracking-widest text-gold-400 select-text">
                    {pin}
                  </span>
                  <button
                    type="button"
                    onClick={() => void handleCopy()}
                    className="btn-tactile rounded p-1 text-text-muted hover:text-text-strong cursor-pointer"
                    title="Copy pairing code"
                  >
                    {copied ? <Check className="h-4 w-4 text-status-success" /> : <Copy className="h-4 w-4" />}
                  </button>
                </div>
              </>
            ) : (
              <>
                <span className="text-[11px] text-text-muted">
                  Enable &ldquo;Allow other PCs to control this PC&rdquo; to generate a pairing code.
                </span>
                <button
                  type="button"
                  disabled={isLoading}
                  onClick={() => void setAllowControl(true)}
                  className="btn-tactile flex items-center gap-1.5 rounded-lg border border-gold-400/50 bg-ink-800 px-3 py-1.5 text-xs font-semibold text-gold-400 hover:bg-ink-700 cursor-pointer disabled:opacity-40"
                >
                  <Power className="h-3.5 w-3.5" />
                  <span>Allow other PCs to control this PC</span>
                </button>
              </>
            )}
          </div>
        )}

        {/* Enter Target Pairing Code (if linking peer) */}
        {selectedPeer && (
          <div className="space-y-2 rounded-xl border border-ink-700 bg-ink-950 p-4">
            <span className="text-[11px] text-text font-medium">
              Enter pairing code shown on {selectedPeer.name}:
            </span>
            <div className="flex items-center gap-2">
              <input
                type="text"
                placeholder="6-digit code"
                maxLength={6}
                value={targetPin}
                onChange={(e) => setTargetPin(e.target.value)}
                className="h-9 flex-1 rounded-lg border border-ink-700 bg-ink-800 px-3 font-mono text-sm tracking-widest text-text-strong outline-none focus:border-gold-400"
              />
              <button
                type="button"
                disabled={targetPin.length < 6 || isVerifying}
                onClick={() => void handleVerifyRemote()}
                className="btn-tactile flex items-center gap-1.5 rounded-lg bg-gold-400 px-3 py-2 text-xs font-semibold text-ink-950 hover:bg-gold-600 disabled:opacity-40 cursor-pointer"
              >
                <span>{verifiedSuccess ? "Verified" : "Pair"}</span>
                {verifiedSuccess ? <Check className="h-3.5 w-3.5" /> : <ArrowRight className="h-3.5 w-3.5" />}
              </button>
            </div>
            <button
              type="button"
              onClick={() => void forgetPeer(selectedPeer.id).then(closeModal)}
              className="btn-tactile text-[11px] text-status-danger hover:opacity-80 cursor-pointer"
            >
              Forget
            </button>
          </div>
        )}

        <div className="rounded-xl border border-ink-700 bg-ink-950 p-3 text-xs text-text-muted space-y-1.5">
          <div className="flex items-center gap-1.5 font-medium text-text">
            <ShieldCheck className="h-3.5 w-3.5 text-status-success" />
            <span>Encrypted Direct LAN Connection (QUIC)</span>
          </div>
          <p className="text-[11px] text-text-muted leading-relaxed">
            Traffic stays strictly on your local network (UDP port 4177). Zero cloud relay.
          </p>
        </div>

        <div className="flex justify-end pt-1">
          <button
            type="button"
            onClick={closeModal}
            className="btn-tactile rounded-lg border border-ink-700 bg-ink-800 px-4 py-2 text-xs font-semibold text-text-strong hover:border-text-muted cursor-pointer"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
