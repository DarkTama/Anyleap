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
import { usePresence } from "@/lib/usePresence";

function StatusIcon({ status }: { status: DoctorStatus }) {
  switch (status) {
    case "pass":
      return <CheckCircle2 className="h-4 w-4 shrink-0 text-status-success" />;
    case "warn":
      return <AlertTriangle className="h-4 w-4 shrink-0 text-status-warning" />;
    case "fail":
      return <XCircle className="h-4 w-4 shrink-0 text-status-danger" />;
    case "skip":
      return <MinusCircle className="h-4 w-4 shrink-0 text-text-muted" />;
  }
}

export function ConnectionDoctor() {
  const open = useAppStore((s) => s.doctorOpen);
  const serial = useAppStore((s) => s.doctorSerial);
  const closeDoctor = useAppStore((s) => s.closeDoctor);
  const { mounted, closing } = usePresence(open, 140);

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

  if (!mounted) return null;

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
      data-state={closing ? "closing" : "open"}
      className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4 select-none"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeDoctor();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Connection Doctor"
        data-state={closing ? "closing" : "open"}
        className="modal-panel flex w-full max-w-lg flex-col overflow-hidden rounded-xl border border-ink-700 bg-ink-900 text-text shadow-elevated"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink-700 px-4 py-3">
          <div className="flex items-center gap-2">
            <Stethoscope className="h-4 w-4 text-mist-300" />
            <div>
              <h2 className="text-sm font-semibold text-text-strong">Connection Doctor</h2>
              <p className="font-mono text-[10px] text-text-muted">
                {serial ? `Target: ${serial}` : "Environment & ADB diagnostics"}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={closeDoctor}
            className="btn-tactile rounded-md p-1 text-text-muted hover:bg-ink-800 hover:text-text-strong cursor-pointer"
            aria-label="Close Connection Doctor"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Body */}
        <div className="max-h-96 space-y-2 overflow-y-auto p-4">
          {error && (
            <div className="rounded-lg border border-status-danger/40 bg-status-danger-bg px-3 py-2 text-xs text-status-danger">
              {error}
            </div>
          )}

          {running && checks.length === 0 ? (
            <div className="flex items-center justify-center gap-2 py-10 text-xs text-text-muted">
              <RefreshCw className="h-4 w-4 animate-spin text-gold-400" />
              <span>Running diagnostics...</span>
            </div>
          ) : (
            checks.map((c) => (
              <div
                key={c.id}
                className={`rounded-lg border px-3 py-2.5 ${
                  c.status === "fail"
                    ? "border-status-danger/40 bg-status-danger-bg"
                    : c.status === "warn"
                      ? "border-status-warning/40 bg-status-warning-bg"
                      : "border-ink-700 bg-ink-950"
                }`}
              >
                <div className="flex items-start gap-2.5">
                  <div className="mt-0.5">
                    <StatusIcon status={c.status} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-xs font-semibold text-text-strong">
                        {c.label}
                      </span>
                      <span className="font-mono text-[10px] uppercase text-text-muted">
                        {c.status}
                      </span>
                    </div>
                    <p className="mt-0.5 text-[11px] text-text break-words">
                      {c.detail}
                    </p>
                    {c.fixHint && (
                      <p className="mt-1 rounded bg-ink-900 px-2 py-1 text-[11px] text-status-warning border border-ink-700">
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
        <div className="flex items-center justify-between border-t border-ink-700 px-4 py-3">
          <button
            type="button"
            disabled={checks.length === 0}
            onClick={() => void handleCopyReport()}
            className="btn-tactile flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong cursor-pointer disabled:opacity-40"
          >
            {copied ? (
              <>
                <Check className="h-3.5 w-3.5 text-status-success" />
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
            className="btn-tactile flex items-center gap-1.5 rounded-lg bg-gold-400 px-3.5 py-1.5 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer disabled:opacity-50"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${running ? "animate-spin" : ""}`} />
            <span>{running ? "Running..." : "Run again"}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
