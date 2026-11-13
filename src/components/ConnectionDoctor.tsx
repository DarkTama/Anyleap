import { useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  Copy,
  MinusCircle,
  RefreshCw,
  Stethoscope,
  X,
  XCircle,
} from "lucide-react";
import { runConnectionDoctor } from "@/lib/tauri";
import { useAppStore } from "@/store/useAppStore";
import type { DoctorCheck, DoctorStatus } from "@/lib/types";

function StatusIcon({ status }: { status: DoctorStatus }) {
  switch (status) {
    case "pass":
      return <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-400" />;
    case "warn":
      return <AlertTriangle className="h-4 w-4 shrink-0 text-amber-400" />;
    case "fail":
      return <XCircle className="h-4 w-4 shrink-0 text-rose-400" />;
    case "skip":
      return <MinusCircle className="h-4 w-4 shrink-0 text-zinc-500" />;
  }
}

export function ConnectionDoctor() {
  const open = useAppStore((s) => s.doctorOpen);
  const serial = useAppStore((s) => s.doctorSerial);
  const closeDoctor = useAppStore((s) => s.closeDoctor);

  const [checks, setChecks] = useState<DoctorCheck[]>([]);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const run = useCallback(async () => {
    setRunning(true);
    setError(null);
    setCopied(false);
    try {
      const result = await runConnectionDoctor(serial);
      setChecks(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  }, [serial]);

  useEffect(() => {
    if (open) {
      void run();
    }
  }, [open, run]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closeDoctor();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, closeDoctor]);

  if (!open) return null;

  const handleCopyReport = async () => {
    const header = `AnyLeap Connection Doctor${serial ? ` (${serial})` : ""}\n${new Date().toISOString()}\n`;
    const body = checks
      .map((c) => {
        const line = `[${c.status.toUpperCase()}] ${c.label}: ${c.detail}`;
        return c.fixHint ? `${line}\n  Fix: ${c.fixHint}` : line;
      })
      .join("\n");
    try {
      await navigator.clipboard.writeText(`${header}\n${body}`);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard write may fail if unfocused
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-xs select-none"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeDoctor();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Connection Doctor"
        className="flex w-full max-w-lg flex-col overflow-hidden rounded-xl border border-[#27272a] bg-[#121215] text-zinc-100 shadow-2xl"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[#222226] px-4 py-3">
          <div className="flex items-center gap-2">
            <Stethoscope className="h-4 w-4 text-zinc-300" />
            <div>
              <h2 className="text-sm font-semibold">Connection Doctor</h2>
              <p className="font-mono text-[10px] text-zinc-400">
                {serial ? `Target: ${serial}` : "Environment & ADB diagnostics"}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={closeDoctor}
            className="rounded-md p-1 text-zinc-400 hover:bg-[#1a1a20] hover:text-zinc-100 cursor-pointer"
            aria-label="Close Connection Doctor"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Body */}
        <div className="max-h-96 space-y-2 overflow-y-auto p-4">
          {error && (
            <div className="rounded-lg border border-rose-500/40 bg-rose-950/60 px-3 py-2 text-xs text-rose-200">
              {error}
            </div>
          )}

          {running && checks.length === 0 ? (
            <div className="flex items-center justify-center gap-2 py-10 text-xs text-zinc-400">
              <RefreshCw className="h-4 w-4 animate-spin" />
              <span>Running diagnostics...</span>
            </div>
          ) : (
            checks.map((c) => (
              <div
                key={c.id}
                className={`rounded-lg border px-3 py-2.5 ${
                  c.status === "fail"
                    ? "border-rose-500/30 bg-rose-950/20"
                    : c.status === "warn"
                      ? "border-amber-500/30 bg-amber-950/20"
                      : "border-[#222226] bg-[#16161a]"
                }`}
              >
                <div className="flex items-start gap-2.5">
                  <div className="mt-0.5">
                    <StatusIcon status={c.status} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-xs font-semibold text-zinc-100">
                        {c.label}
                      </span>
                      <span className="font-mono text-[10px] uppercase text-zinc-400">
                        {c.status}
                      </span>
                    </div>
                    <p className="mt-0.5 text-[11px] text-zinc-300 break-words">
                      {c.detail}
                    </p>
                    {c.fixHint && (
                      <p className="mt-1 rounded bg-[#111114] px-2 py-1 text-[11px] text-amber-300/90 border border-[#27272a]">
                        Fix: {c.fixHint}
                      </p>
                    )}
                  </div>
                </div>
              </div>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[#222226] px-4 py-3">
          <button
            type="button"
            disabled={checks.length === 0}
            onClick={() => void handleCopyReport()}
            className="btn-tactile flex items-center gap-1.5 rounded-lg border border-[#27272a] bg-[#16161a] px-3 py-1.5 text-xs font-medium text-zinc-300 hover:border-zinc-600 hover:bg-[#1c1c21] cursor-pointer disabled:opacity-40"
          >
            {copied ? (
              <>
                <Check className="h-3.5 w-3.5 text-emerald-400" />
                <span>Copied</span>
              </>
            ) : (
              <>
                <Copy className="h-3.5 w-3.5" />
                <span>Copy report</span>
              </>
            )}
          </button>

          <button
            type="button"
            disabled={running}
            onClick={() => void run()}
            className="btn-tactile flex items-center gap-1.5 rounded-lg bg-zinc-100 px-3.5 py-1.5 text-xs font-semibold text-zinc-950 hover:bg-zinc-200 cursor-pointer disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${running ? "animate-spin" : ""}`} />
            <span>{running ? "Running..." : "Run again"}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
