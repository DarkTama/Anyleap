import { useState } from "react";
import { X, KeyRound, Copy, Check, ShieldCheck, ArrowRight } from "lucide-react";
import { useKvmStore } from "@/stores/kvmStore";

export function PairingPinModal() {
  const isModalOpen = useKvmStore((s) => s.isPairingModalOpen);
  const closeModal = useKvmStore((s) => s.closePairingModal);
  const status = useKvmStore((s) => s.status);
  const selectedPeer = useKvmStore((s) => s.selectedPeer);
  const [copied, setCopied] = useState(false);
  const [targetPin, setTargetPin] = useState("");
  const [isVerifying, setIsVerifying] = useState(false);
  const [verifiedSuccess, setVerifiedSuccess] = useState(false);

  if (!isModalOpen) return null;

  const handleCopy = () => {
    if (status.security_pin) {
      navigator.clipboard.writeText(status.security_pin);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleVerifyRemote = async () => {
    if (!targetPin.trim() || !selectedPeer) return;
    setIsVerifying(true);
    try {
      const res = await fetch(`http://${selectedPeer.ip}:${selectedPeer.tcp_port}/pair/confirm`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          computer_id: status.local_id || "local-client",
          pin: targetPin.trim(),
        }),
      });
      const data = await res.json();
      if (data.success) {
        setVerifiedSuccess(true);
        setTimeout(() => {
          closeModal();
        }, 1200);
      } else {
        alert("Incorrect PIN entered");
      }
    } catch (e) {
      alert(`Connection failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setIsVerifying(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4">
      <div className="relative w-full max-w-md rounded-2xl border border-[#222226] bg-[#141417] shadow-2xl p-6 space-y-5">
        <div className="flex items-center justify-between border-b border-[#222226] pb-3">
          <div className="flex items-center gap-2">
            <div className="grid h-8 w-8 place-items-center rounded-lg bg-cyan-500/10 text-cyan-400 border border-cyan-500/20">
              <KeyRound className="h-4 w-4" />
            </div>
            <div>
              <h2 className="text-sm font-semibold text-zinc-100">KVM Security PIN</h2>
              <p className="text-[11px] text-zinc-400">
                {selectedPeer ? `Pairing with ${selectedPeer.name}` : "Local-first mutual device verification"}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={closeModal}
            className="rounded-lg p-1 text-zinc-400 hover:bg-[#202026] hover:text-zinc-200 cursor-pointer"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* This Computer's PIN */}
        <div className="flex flex-col items-center justify-center space-y-2 rounded-xl border border-[#27272a] bg-[#101013] p-4">
          <span className="text-[11px] text-zinc-400">This PC PIN (Share with other computer):</span>
          <div className="flex items-center gap-2 rounded-lg border border-cyan-500/40 bg-[#161a24] px-4 py-2">
            <span className="font-mono text-xl font-black tracking-widest text-cyan-300">
              {status.security_pin || "------"}
            </span>
            <button
              type="button"
              onClick={handleCopy}
              className="btn-tactile rounded p-1 text-zinc-400 hover:text-white cursor-pointer"
              title="Copy PIN"
            >
              {copied ? <Check className="h-4 w-4 text-emerald-400" /> : <Copy className="h-4 w-4" />}
            </button>
          </div>
        </div>

        {/* Enter Target PIN (if linking peer) */}
        {selectedPeer && (
          <div className="space-y-2 rounded-xl border border-[#27272a] bg-[#101013] p-4">
            <span className="text-[11px] text-zinc-300 font-medium">
              Enter PIN shown on {selectedPeer.name}:
            </span>
            <div className="flex items-center gap-2">
              <input
                type="text"
                placeholder="6-digit PIN"
                maxLength={6}
                value={targetPin}
                onChange={(e) => setTargetPin(e.target.value)}
                className="h-9 flex-1 rounded-lg border border-[#27272a] bg-[#18181c] px-3 font-mono text-sm tracking-widest text-zinc-100 outline-none focus:border-cyan-500"
              />
              <button
                type="button"
                disabled={targetPin.length < 6 || isVerifying}
                onClick={() => void handleVerifyRemote()}
                className="btn-tactile flex items-center gap-1.5 rounded-lg bg-cyan-500 px-3 py-2 text-xs font-semibold text-zinc-950 hover:bg-cyan-400 disabled:opacity-40 cursor-pointer"
              >
                <span>{verifiedSuccess ? "Verified" : "Authorize"}</span>
                {verifiedSuccess ? <Check className="h-3.5 w-3.5" /> : <ArrowRight className="h-3.5 w-3.5" />}
              </button>
            </div>
          </div>
        )}

        <div className="rounded-xl border border-[#222226] bg-[#0c0c0e] p-3 text-xs text-zinc-400 space-y-1.5">
          <div className="flex items-center gap-1.5 font-medium text-zinc-300">
            <ShieldCheck className="h-3.5 w-3.5 text-emerald-400" />
            <span>Encrypted Direct LAN Connection</span>
          </div>
          <p className="text-[11px] text-zinc-500 leading-relaxed">
            Traffic stays strictly on your local network (ports 4177/4178). Zero cloud relay.
          </p>
        </div>

        <div className="flex justify-end pt-1">
          <button
            type="button"
            onClick={closeModal}
            className="btn-tactile rounded-lg bg-white px-4 py-2 text-xs font-semibold text-zinc-950 hover:bg-zinc-200 cursor-pointer"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
