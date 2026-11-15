import { describe, it, expect } from "vitest";
import {
  initialTransferBarState,
  upsertTransferProgress,
  failTransfer,
  setTransferNotice,
  expireTransferBar,
  dismissTransferItem,
  dismissTransferNotice,
  getVisibleTransfers,
  getTransferKindLabel,
  formatTransferBytes,
  TRANSFER_AWAITING_FETCH_TTL_MS,
  TRANSFER_COMPLETED_LINGER_MS,
  TRANSFER_NOTICE_LINGER_MS,
  TRANSFER_STALE_DRAG_MS,
} from "./transferBarReducer";

describe("transferBarReducer", () => {
  it("adds a new active transfer and updates its progress in place", () => {
    let state = upsertTransferProgress(
      initialTransferBarState,
      { id: "t1", kind: "clip", name: "archive.zip", pct: 25 },
      1000,
    );
    expect(state.transfers).toHaveLength(1);
    expect(state.transfers[0]).toMatchObject({
      id: "t1",
      kind: "clip",
      name: "archive.zip",
      pct: 25,
      status: "active",
      expiresAt: null,
    });

    state = upsertTransferProgress(
      state,
      { id: "t1", kind: "clip", name: "archive.zip", pct: 74.6 },
      2000,
    );
    expect(state.transfers).toHaveLength(1);
    expect(state.transfers[0].pct).toBe(75);
    expect(state.transfers[0].status).toBe("active");
    expect(state.transfers[0].expiresAt).toBeNull();
  });

  it("marks transfer completed at 100% or state='done' and schedules 1.5s linger expiry", () => {
    let state = upsertTransferProgress(
      initialTransferBarState,
      { id: "t1", kind: "open", name: "report.pdf", pct: 40, state: "active" },
      1000,
    );
    state = upsertTransferProgress(
      state,
      { id: "t1", kind: "open", name: "report.pdf", pct: 120, state: "done" },
      2500,
    );

    expect(state.transfers[0]).toMatchObject({
      pct: 100,
      status: "done",
      expiresAt: 2500 + TRANSFER_COMPLETED_LINGER_MS,
    });
    expect(getTransferKindLabel("clip", "active")).toBe("Copying");
    expect(getTransferKindLabel("drag", "active")).toBe("Dragging");
    expect(getTransferKindLabel("open", "active")).toBe("Opening");
    expect(getTransferKindLabel("clip", "done")).toBe("Ready to paste");
    expect(getTransferKindLabel("open", "done")).toBe("Opened");
    expect(getTransferKindLabel("drag", "done")).toBe("Transferred");
    expect(getTransferKindLabel("clip", "awaiting_fetch")).toBe("Large file");
    expect(getTransferKindLabel("clip", "cancelled")).toBe("Cancelled");
  });

  it("handles awaiting_fetch, failed, and cancelled states with proper TTLs", () => {
    let state = upsertTransferProgress(
      initialTransferBarState,
      {
        id: "offer-1",
        kind: "clip",
        name: "dataset.tar.gz",
        pct: 0,
        state: "awaiting_fetch",
        sizeBytes: 256 * 1024 * 1024,
      },
      1000,
    );
    expect(state.transfers[0]).toMatchObject({
      id: "offer-1",
      status: "awaiting_fetch",
      pct: 0,
      sizeBytes: 256 * 1024 * 1024,
      expiresAt: 1000 + TRANSFER_AWAITING_FETCH_TTL_MS,
    });

    // Clicking Fetch transitions it to active (preserves sizeBytes if omitted)
    state = upsertTransferProgress(
      state,
      { id: "offer-1", kind: "clip", name: "dataset.tar.gz", pct: 25, state: "active" },
      2000,
    );
    expect(state.transfers[0]).toMatchObject({
      status: "active",
      pct: 25,
      sizeBytes: 256 * 1024 * 1024,
      expiresAt: null,
    });

    // Cancelling sets state="cancelled" and 4s linger expiry
    state = upsertTransferProgress(
      state,
      { id: "offer-1", kind: "clip", name: "dataset.tar.gz", pct: 25, state: "cancelled" },
      3000,
    );
    expect(state.transfers[0]).toMatchObject({
      status: "cancelled",
      expiresAt: 3000 + TRANSFER_NOTICE_LINGER_MS,
    });

    // Failed via payload state sets 4s linger expiry
    state = upsertTransferProgress(
      state,
      { id: "t2", kind: "drag", name: "video.mp4", pct: 40, state: "failed" },
      3500,
    );
    expect(state.transfers[1]).toMatchObject({
      status: "failed",
      expiresAt: 3500 + TRANSFER_NOTICE_LINGER_MS,
    });

    // Before 4s: cancelled and failed remain; at 3000+4000 cancelled expires
    state = expireTransferBar(state, 3000 + TRANSFER_NOTICE_LINGER_MS);
    expect(state.transfers.map((t) => t.id)).toEqual(["t2"]);
  });

  it("formats transfer byte sizes into human-readable strings", () => {
    expect(formatTransferBytes(undefined)).toBe("");
    expect(formatTransferBytes(512)).toBe("512 B");
    expect(formatTransferBytes(2048)).toBe("2.0 KB");
    expect(formatTransferBytes(142.5 * 1024 * 1024)).toBe("142.5 MB");
    expect(formatTransferBytes(1.5 * 1024 * 1024 * 1024)).toBe("1.50 GB");
  });

  it("marks transfer failed and schedules 4s notice expiry", () => {
    let state = upsertTransferProgress(
      initialTransferBarState,
      { id: "t1", kind: "drag", name: "video.mp4", pct: 55 },
      1000,
    );
    state = failTransfer(state, "t1", "Staging limit exceeded", 2000);

    expect(state.transfers[0].status).toBe("failed");
    expect(state.transfers[0].errorText).toBe("Staging limit exceeded");
    expect(state.transfers[0].expiresAt).toBe(2000 + TRANSFER_NOTICE_LINGER_MS);
    expect(getTransferKindLabel("drag", "failed")).toBe("Failed");

    // Unknown id is a no-op
    expect(failTransfer(state, "missing", "err", 2000)).toBe(state);
  });

  it("expires completed/failed transfers and notices while keeping active ones", () => {
    let state = upsertTransferProgress(
      initialTransferBarState,
      { id: "active", kind: "drag", name: "big.iso", pct: 30 },
      1000,
    );
    state = upsertTransferProgress(
      state,
      { id: "done", kind: "clip", name: "photo.png", pct: 100 },
      1000,
    );
    state = setTransferNotice(state, { text: "Transfer failed: disk full" }, 1000);

    // Before 1.5s: both transfers and notice remain
    state = expireTransferBar(state, 1000 + TRANSFER_COMPLETED_LINGER_MS - 1);
    expect(state.transfers).toHaveLength(2);
    expect(state.notice).not.toBeNull();

    // At 1.5s: completed transfer expires, active transfer and 4s notice remain
    state = expireTransferBar(state, 1000 + TRANSFER_COMPLETED_LINGER_MS);
    expect(state.transfers.map((t) => t.id)).toEqual(["active"]);
    expect(state.notice).not.toBeNull();

    // At 4s: notice expires, active transfer still remains
    state = expireTransferBar(state, 1000 + TRANSFER_NOTICE_LINGER_MS);
    expect(state.transfers.map((t) => t.id)).toEqual(["active"]);
    expect(state.notice).toBeNull();
  });

  it("caps visible transfers at 3 and reports overflow count", () => {
    let state = initialTransferBarState;
    for (let i = 1; i <= 5; i++) {
      state = upsertTransferProgress(
        state,
        { id: `t${i}`, kind: "clip", name: `file-${i}.txt`, pct: i * 10 },
        1000,
      );
    }

    const { visible, overflowCount } = getVisibleTransfers(state.transfers);
    expect(visible.map((t) => t.id)).toEqual(["t1", "t2", "t3"]);
    expect(overflowCount).toBe(2);
  });

  it("supports dismissing individual transfers and notices", () => {
    let state = upsertTransferProgress(
      initialTransferBarState,
      { id: "t1", kind: "clip", name: "a.txt", pct: 50 },
      1000,
    );
    state = setTransferNotice(state, { text: "Warning" }, 1000);

    state = dismissTransferItem(state, "t1");
    expect(state.transfers).toHaveLength(0);

    state = dismissTransferNotice(state);
    expect(state.notice).toBeNull();
  });

  it("auto-dismisses drag rows on done/failed/cancelled and expires stale active drag rows 15s after reaching 100%", () => {
    // Terminal states on drag rows auto-dismiss with the same TTLs as clip
    let state = upsertTransferProgress(
      initialTransferBarState,
      { id: "d-done", kind: "drag", name: "a.png", pct: 100, state: "done" },
      1000,
    );
    state = upsertTransferProgress(
      state,
      { id: "d-fail", kind: "drag", name: "b.png", pct: 60, state: "failed" },
      1000,
    );
    state = upsertTransferProgress(
      state,
      { id: "d-cancel", kind: "drag", name: "c.png", pct: 20, state: "cancelled" },
      1000,
    );
    expect(state.transfers.map((t) => t.expiresAt)).toEqual([
      1000 + TRANSFER_COMPLETED_LINGER_MS,
      1000 + TRANSFER_NOTICE_LINGER_MS,
      1000 + TRANSFER_NOTICE_LINGER_MS,
    ]);

    state = expireTransferBar(state, 1000 + TRANSFER_COMPLETED_LINGER_MS);
    expect(state.transfers.map((t) => t.id)).toEqual(["d-fail", "d-cancel"]);
    state = expireTransferBar(state, 1000 + TRANSFER_NOTICE_LINGER_MS);
    expect(state.transfers).toHaveLength(0);

    // Defensive stale-row rule: drag row reaching 100% while still state="active" expires after 15s of no updates
    let staleState = upsertTransferProgress(
      initialTransferBarState,
      { id: "d-stale", kind: "drag", name: "huge.iso", pct: 100, state: "active" },
      5000,
    );
    expect(staleState.transfers[0].expiresAt).toBe(5000 + TRANSFER_STALE_DRAG_MS);

    // Before 15s: still present
    staleState = expireTransferBar(staleState, 5000 + TRANSFER_STALE_DRAG_MS - 1);
    expect(staleState.transfers).toHaveLength(1);

    // A progress/state update before 15s resets the 15s window
    staleState = upsertTransferProgress(
      staleState,
      { id: "d-stale", kind: "drag", name: "huge.iso", pct: 100, state: "active" },
      10000,
    );
    expect(staleState.transfers[0].expiresAt).toBe(10000 + TRANSFER_STALE_DRAG_MS);
    staleState = expireTransferBar(staleState, 5000 + TRANSFER_STALE_DRAG_MS);
    expect(staleState.transfers).toHaveLength(1);

    // After 15s with no further update: dismissed locally
    staleState = expireTransferBar(staleState, 10000 + TRANSFER_STALE_DRAG_MS);
    expect(staleState.transfers).toHaveLength(0);
  });
});
