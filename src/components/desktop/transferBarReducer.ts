import type {
  KvmContinuityNoticePayload,
  KvmTransferProgressPayload,
  TransferKind,
  TransferProgressState,
} from "@/types/kvm";

export const TRANSFER_COMPLETED_LINGER_MS = 1500;
export const TRANSFER_NOTICE_LINGER_MS = 4000;
export const TRANSFER_AWAITING_FETCH_TTL_MS = 10 * 60 * 1000;
export const TRANSFER_STALE_DRAG_MS = 15_000;
export const TRANSFER_CANCEL_FALLBACK_MS = 3_000;
export const TRANSFER_BAR_MAX_VISIBLE = 3;

export type TransferStatus = TransferProgressState;

export interface TransferBarItem {
  id: string;
  kind: TransferKind;
  name: string;
  pct: number;
  status: TransferStatus;
  sizeBytes?: number;
  expiresAt: number | null;
  errorText?: string;
}

export interface TransferBarNotice {
  id: string;
  text: string;
  expiresAt: number;
}

export interface TransferBarState {
  transfers: TransferBarItem[];
  notice: TransferBarNotice | null;
}

export const initialTransferBarState: TransferBarState = {
  transfers: [],
  notice: null,
};

function resolveTransferStatus(
  payload: KvmTransferProgressPayload,
  clampedPct: number,
): TransferStatus {
  if (payload.state) {
    return payload.state;
  }
  return clampedPct >= 100 ? "done" : "active";
}

function resolveExpiresAt(
  status: TransferStatus,
  kind: TransferKind,
  pct: number,
  now: number,
): number | null {
  switch (status) {
    case "done":
      return now + TRANSFER_COMPLETED_LINGER_MS;
    case "failed":
    case "cancelled":
      return now + TRANSFER_NOTICE_LINGER_MS;
    case "awaiting_fetch":
      return now + TRANSFER_AWAITING_FETCH_TTL_MS;
    case "active":
      if (kind === "drag" && pct >= 100) {
        return now + TRANSFER_STALE_DRAG_MS;
      }
      return null;
  }
}

export function upsertTransferProgress(
  state: TransferBarState,
  payload: KvmTransferProgressPayload,
  now = Date.now(),
): TransferBarState {
  const clampedPct = Math.max(0, Math.min(100, Math.round(payload.pct)));
  const status = resolveTransferStatus(payload, clampedPct);
  const pct =
    status === "awaiting_fetch" ? 0 : status === "done" ? 100 : clampedPct;

  const idx = state.transfers.findIndex((t) => t.id === payload.id);
  const existing = idx >= 0 ? state.transfers[idx] : undefined;
  const sizeBytes =
    typeof payload.sizeBytes === "number" && Number.isFinite(payload.sizeBytes)
      ? Math.max(0, payload.sizeBytes)
      : existing?.sizeBytes;

  const nextItem: TransferBarItem = {
    id: payload.id,
    kind: payload.kind,
    name: payload.name,
    pct,
    status,
    sizeBytes,
    expiresAt: resolveExpiresAt(status, payload.kind, pct, now),
  };

  if (idx >= 0) {
    const nextTransfers = [...state.transfers];
    nextTransfers[idx] = nextItem;
    return { ...state, transfers: nextTransfers };
  }
  return { ...state, transfers: [...state.transfers, nextItem] };
}

export function failTransfer(
  state: TransferBarState,
  id: string,
  errorText?: string,
  now = Date.now(),
): TransferBarState {
  const idx = state.transfers.findIndex((t) => t.id === id);
  if (idx < 0) return state;

  const nextTransfers = [...state.transfers];
  nextTransfers[idx] = {
    ...nextTransfers[idx],
    status: "failed",
    expiresAt: now + TRANSFER_NOTICE_LINGER_MS,
    errorText: errorText?.trim() || undefined,
  };
  return { ...state, transfers: nextTransfers };
}

export function setTransferNotice(
  state: TransferBarState,
  payload: KvmContinuityNoticePayload,
  now = Date.now(),
): TransferBarState {
  const text = payload.text.trim();
  if (!text) return state;
  return {
    ...state,
    notice: {
      id: `notice-${now}`,
      text,
      expiresAt: now + TRANSFER_NOTICE_LINGER_MS,
    },
  };
}

export function expireTransferBar(
  state: TransferBarState,
  now = Date.now(),
): TransferBarState {
  const transfers = state.transfers.filter(
    (t) => t.expiresAt === null || t.expiresAt > now,
  );
  const notice =
    state.notice !== null && state.notice.expiresAt > now ? state.notice : null;

  if (
    transfers.length === state.transfers.length &&
    notice === state.notice
  ) {
    return state;
  }
  return { transfers, notice };
}

export function dismissTransferItem(
  state: TransferBarState,
  id: string,
): TransferBarState {
  const transfers = state.transfers.filter((t) => t.id !== id);
  if (transfers.length === state.transfers.length) return state;
  return { ...state, transfers };
}

export function dismissTransferNotice(state: TransferBarState): TransferBarState {
  if (state.notice === null) return state;
  return { ...state, notice: null };
}

export function getVisibleTransfers(
  transfers: TransferBarItem[],
  maxVisible = TRANSFER_BAR_MAX_VISIBLE,
): { visible: TransferBarItem[]; overflowCount: number } {
  const safeMax = Math.max(0, maxVisible);
  const visible = transfers.slice(0, safeMax);
  const overflowCount = Math.max(0, transfers.length - visible.length);
  return { visible, overflowCount };
}

export function getTransferKindLabel(
  kind: TransferKind,
  status: TransferStatus,
): string {
  if (status === "failed") return "Failed";
  if (status === "cancelled") return "Cancelled";
  if (status === "awaiting_fetch") return "Large file";
  if (status === "done") {
    switch (kind) {
      case "clip":
        return "Ready to paste";
      case "open":
        return "Opened";
      case "drag":
        return "Transferred";
    }
  }
  switch (kind) {
    case "clip":
      return "Copying";
    case "drag":
      return "Dragging";
    case "open":
      return "Opening";
  }
}

export function formatTransferBytes(bytes?: number): string {
  if (bytes === undefined || !Number.isFinite(bytes) || bytes < 0) {
    return "";
  }
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) {
    const kb = bytes / 1024;
    return `${kb.toFixed(kb < 10 ? 1 : 0)} KB`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
