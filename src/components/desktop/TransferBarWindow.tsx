import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AlertTriangle, Check, Download, FolderOpen, Laptop, X } from "lucide-react";
import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  kvmCancelTransfer,
  kvmFetchClip,
  listenKvmContinuityNotice,
  listenKvmTransferProgress,
} from "@/services/kvmService";
import type { KvmTransferProgressPayload } from "@/types/kvm";
import {
  TRANSFER_CANCEL_FALLBACK_MS,
  dismissTransferItem,
  dismissTransferNotice,
  expireTransferBar,
  formatTransferBytes,
  getTransferKindLabel,
  getVisibleTransfers,
  initialTransferBarState,
  setTransferNotice,
  upsertTransferProgress,
  type TransferBarState,
} from "./transferBarReducer";

const MAX_BAR_WIDTH = 340;
const MARGIN_BOTTOM = 24;

function parseInitialState(): TransferBarState {
  try {
    const params = new URLSearchParams(window.location.search);
    const raw = params.get("init");
    if (!raw) return initialTransferBarState;
    const parsed = JSON.parse(raw) as {
      transfers?: KvmTransferProgressPayload[];
      notice?: string | null;
    };
    const now = Date.now();
    let state = initialTransferBarState;
    if (Array.isArray(parsed.transfers)) {
      for (const t of parsed.transfers) {
        if (t && typeof t.id === "string") {
          state = upsertTransferProgress(state, t, now);
        }
      }
    }
    if (typeof parsed.notice === "string" && parsed.notice.trim()) {
      state = setTransferNotice(state, { text: parsed.notice }, now);
    }
    return state;
  } catch {
    return initialTransferBarState;
  }
}

export function TransferBarWindow() {
  const [state, setState] = useState<TransferBarState>(parseInitialState);
  const hadContentRef = useRef(
    state.transfers.length > 0 || state.notice !== null,
  );
  const contentRef = useRef<HTMLDivElement>(null);
  const lastSizeRef = useRef<{ width: number; height: number }>({ width: 0, height: 0 });
  const cancelTimersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  const handleCancelTransfer = (id: string) => {
    const existing = cancelTimersRef.current.get(id);
    if (existing) clearTimeout(existing);

    const removeLocally = () => {
      const timer = cancelTimersRef.current.get(id);
      if (timer) {
        clearTimeout(timer);
        cancelTimersRef.current.delete(id);
      }
      setState((cur) => dismissTransferItem(cur, id));
    };

    const fallbackTimer = setTimeout(removeLocally, TRANSFER_CANCEL_FALLBACK_MS);
    cancelTimersRef.current.set(id, fallbackTimer);

    void kvmCancelTransfer(id).then(removeLocally, () => {});
  };

  useEffect(() => {
    const unlistenTransfer = listenKvmTransferProgress((payload) => {
      hadContentRef.current = true;
      setState((cur) => upsertTransferProgress(cur, payload, Date.now()));
    });

    const unlistenNotice = listenKvmContinuityNotice((payload) => {
      if (!payload.text.trim()) return;
      hadContentRef.current = true;
      setState((cur) => setTransferNotice(cur, payload, Date.now()));
    });

    const timer = setInterval(() => {
      setState((cur) => expireTransferBar(cur, Date.now()));
    }, 250);

    return () => {
      clearInterval(timer);
      for (const cancelTimer of cancelTimersRef.current.values()) {
        clearTimeout(cancelTimer);
      }
      cancelTimersRef.current.clear();
      void unlistenTransfer.then((f) => f());
      void unlistenNotice.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const hasContent = state.transfers.length > 0 || state.notice !== null;
    if (hasContent) {
      hadContentRef.current = true;
      return;
    }
    if (hadContentRef.current) {
      void getCurrentWebviewWindow()
        .close()
        .catch(() => {});
    }
  }, [state.transfers.length, state.notice]);

  const { visible, overflowCount } = getVisibleTransfers(state.transfers);
  const { notice } = state;

  useLayoutEffect(() => {
    const el = contentRef.current;
    if (!el) return;

    const syncWindowSize = () => {
      const rect = el.getBoundingClientRect();
      const width = Math.min(MAX_BAR_WIDTH, Math.max(1, Math.ceil(rect.width)));
      const height = Math.max(1, Math.ceil(rect.height));
      if (
        width === lastSizeRef.current.width &&
        height === lastSizeRef.current.height
      ) {
        return;
      }
      lastSizeRef.current = { width, height };

      const screenW = window.screen.availWidth || 1920;
      const screenH = window.screen.availHeight || 1080;
      const x = Math.max(0, Math.round((screenW - width) / 2));
      const y = Math.max(0, Math.round(screenH - height - MARGIN_BOTTOM));
      const win = getCurrentWebviewWindow();
      void win.setSize(new LogicalSize(width, height)).catch(() => {});
      void win.setPosition(new LogicalPosition(x, y)).catch(() => {});
    };

    syncWindowSize();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(syncWindowSize);
    observer.observe(el);
    return () => observer.disconnect();
  }, [visible.length, overflowCount, notice?.id]);

  if (visible.length === 0 && !notice) {
    return null;
  }

  return (
    <div
      ref={contentRef}
      className="inline-flex w-max max-w-[340px] flex-col items-stretch gap-1.5 select-none overflow-hidden bg-transparent"
    >
      {notice && (
        <div className="hud-notice flex w-full items-center justify-between gap-2 rounded-full border border-status-warning/60 bg-ink-900 px-3 py-1.5 text-[11px] text-status-warning">
          <div className="flex min-w-0 items-center gap-1.5">
            <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-status-warning" />
            <span className="truncate font-medium">{notice.text}</span>
          </div>
          <button
            type="button"
            aria-label="Dismiss notice"
            onClick={() => setState((cur) => dismissTransferNotice(cur))}
            className="btn-tactile shrink-0 rounded-full p-0.5 text-status-warning/80 hover:bg-ink-800 hover:text-status-warning cursor-pointer"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      )}

      {visible.length > 0 && (
        <div className="hud-notice flex w-full flex-col gap-1.5 rounded-2xl border border-gold-400/50 bg-ink-900 px-3 py-2 text-[11px] text-text-strong">
          {visible.map((item) => {
            const label = getTransferKindLabel(item.kind, item.status, item.savedPath);
            const isDone = item.status === "done";
            const isFailed = item.status === "failed";
            const isCancelled = item.status === "cancelled";
            const isAwaitingFetch = item.status === "awaiting_fetch";
            const isActive = item.status === "active";
            const sizeLabel = formatTransferBytes(item.sizeBytes);

            if (isAwaitingFetch) {
              return (
                <div key={item.id} className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-1.5">
                    <Download className="h-3 w-3 shrink-0 text-gold-400" />
                    <span className="shrink-0 font-mono text-[10px] font-semibold uppercase tracking-wide text-gold-400">
                      {label}
                    </span>
                    <span className="truncate font-medium text-text-strong">
                      {item.name}
                    </span>
                    {sizeLabel && (
                      <span className="shrink-0 font-mono text-[10px] text-text-muted">
                        {sizeLabel}
                      </span>
                    )}
                  </div>

                  <div className="flex shrink-0 items-center gap-1">
                    <button
                      type="button"
                      onClick={() => void kvmFetchClip(item.id).catch(() => {})}
                      className="btn-tactile rounded-md bg-gold-400 px-2 py-0.5 font-semibold text-[10px] text-ink-950 hover:bg-gold-300 cursor-pointer"
                    >
                      Fetch
                    </button>
                    <button
                      type="button"
                      title="Cancel on both PCs"
                      onClick={() => handleCancelTransfer(item.id)}
                      className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] text-text-muted hover:text-text-strong cursor-pointer"
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              );
            }

            return (
              <div key={item.id} className="flex flex-col gap-1">
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-1.5">
                    {isDone ? (
                      <Check className="h-3 w-3 shrink-0 text-status-success" />
                    ) : isFailed ? (
                      <AlertTriangle className="h-3 w-3 shrink-0 text-status-warning" />
                    ) : isCancelled ? (
                      <X className="h-3 w-3 shrink-0 text-text-muted" />
                    ) : (
                      <Laptop className="h-3 w-3 shrink-0 text-gold-400" />
                    )}
                    <span
                      className={`shrink-0 font-mono text-[10px] font-semibold uppercase tracking-wide ${
                        isDone
                          ? "text-status-success"
                          : isFailed
                            ? "text-status-warning"
                            : isCancelled
                              ? "text-text-muted"
                              : "text-gold-400"
                      }`}
                    >
                      {label}
                    </span>
                    <span className="truncate font-medium text-text-strong">
                      {item.name}
                    </span>
                  </div>

                  <div className="flex shrink-0 items-center gap-1.5">
                    {isDone && item.savedPath ? (
                      <button
                        type="button"
                        onClick={() => void revealItemInDir(item.savedPath!).catch(() => {})}
                        className="btn-tactile inline-flex items-center gap-1 rounded-md border border-status-success/40 bg-ink-800 px-1.5 py-0.5 text-[10px] font-medium text-text-strong hover:border-status-success cursor-pointer"
                      >
                        <FolderOpen className="h-3 w-3" />
                        Show in folder
                      </button>
                    ) : (
                      <span className="font-mono text-[10px] text-text-muted">
                        {item.pct}%
                      </span>
                    )}
                    {isActive && (
                      <button
                        type="button"
                        title="Cancel on both PCs"
                        onClick={() => handleCancelTransfer(item.id)}
                        className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] text-text-muted hover:text-text-strong cursor-pointer"
                      >
                        Cancel
                      </button>
                    )}
                  </div>
                </div>

                <div className="h-0.5 w-full overflow-hidden rounded-full bg-ink-950">
                  <div
                    className={`progress-bar-fill h-full w-full ${
                      isDone
                        ? "bg-status-success"
                        : isFailed
                          ? "bg-status-warning"
                          : isCancelled
                            ? "bg-ink-600"
                            : "bg-gold-400"
                    }`}
                    style={{ transform: `scaleX(${item.pct / 100})` }}
                  />
                </div>
              </div>
            );
          })}

          {overflowCount > 0 && (
            <div className="text-right font-mono text-[10px] text-text-muted">
              +{overflowCount} more
            </div>
          )}
        </div>
      )}
    </div>
  );
}
