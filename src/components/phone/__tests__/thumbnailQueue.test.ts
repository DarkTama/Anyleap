import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThumbnailQueue } from "../thumbnailQueue";

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("ThumbnailQueue", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("caps in-flight requests at 4 and starts visible tiles before overscan in row order", async () => {
    const batches: Array<Record<string, string>> = [];
    const queue = new ThumbnailQueue({
      maxConcurrent: 4,
      batchIntervalMs: 80,
      onBatch: (u) => batches.push(u),
    });

    const started: string[] = [];
    const pending = new Map<string, ReturnType<typeof deferred<string | null>>>();
    const fetcher = (id: string) => {
      started.push(id);
      const d = deferred<string | null>();
      pending.set(id, d);
      return d.promise;
    };

    // Pass overscan items first in the array, visible items with lower priority numbers.
    queue.syncDesired(
      [
        { id: "over-1", priority: 10000 },
        { id: "over-2", priority: 10001 },
        { id: "vis-1", priority: 0 },
        { id: "vis-2", priority: 1 },
        { id: "vis-3", priority: 2 },
        { id: "vis-4", priority: 3 },
        { id: "vis-5", priority: 4 },
      ],
      fetcher,
    );

    expect(started).toEqual(["vis-1", "vis-2", "vis-3", "vis-4"]);
    expect(queue.getStats().inFlightCount).toBe(4);
    expect(queue.getStats().queuedIds).toEqual(["vis-5", "over-1", "over-2"]);

    // Resolve vis-1 -> vis-5 starts next before overscan.
    pending.get("vis-1")!.resolve("asset://vis-1.jpg");
    await Promise.resolve();
    await Promise.resolve();

    expect(started).toEqual(["vis-1", "vis-2", "vis-3", "vis-4", "vis-5"]);
    queue.destroy();
  });

  it("drops queued requests that leave the viewport before starting and ignores late in-flight results", async () => {
    const batches: Array<Record<string, string>> = [];
    const queue = new ThumbnailQueue({
      maxConcurrent: 4,
      batchIntervalMs: 50,
      onBatch: (u) => batches.push(u),
    });

    const started: string[] = [];
    const pending = new Map<string, ReturnType<typeof deferred<string | null>>>();
    const fetcher = (id: string) => {
      started.push(id);
      const d = deferred<string | null>();
      pending.set(id, d);
      return d.promise;
    };

    queue.syncDesired(
      Array.from({ length: 8 }, (_, i) => ({ id: `t-${i}`, priority: i })),
      fetcher,
    );
    expect(started).toEqual(["t-0", "t-1", "t-2", "t-3"]);

    // Scroll far away: only t-20..t-23 are now desired; t-4..t-7 were queued and never started,
    // while t-0..t-3 are already in-flight.
    queue.syncDesired(
      Array.from({ length: 4 }, (_, i) => ({ id: `t-${20 + i}`, priority: i })),
      fetcher,
    );

    expect(queue.getStats().queuedIds).toEqual(["t-20", "t-21", "t-22", "t-23"]);

    // Late resolution for t-0 (now unmounted/off-screen) must be ignored, while freeing slot for t-20.
    pending.get("t-0")!.resolve("asset://t-0.jpg");
    await Promise.resolve();
    await Promise.resolve();

    expect(started).toContain("t-20");
    expect(started).not.toContain("t-4");
    expect(started).not.toContain("t-5");

    vi.advanceTimersByTime(100);
    expect(batches).toEqual([]);

    // Resolving t-20 (still desired) emits batch update.
    pending.get("t-20")!.resolve("asset://t-20.jpg");
    await Promise.resolve();
    await Promise.resolve();
    vi.advanceTimersByTime(60);

    expect(batches).toEqual([{ "t-20": "asset://t-20.jpg" }]);
    queue.destroy();
  });

  it("batches multiple completions within the flush window into a single onBatch call", async () => {
    const batches: Array<Record<string, string>> = [];
    const queue = new ThumbnailQueue({
      maxConcurrent: 4,
      batchIntervalMs: 80,
      onBatch: (u) => batches.push(u),
    });

    const pending = new Map<string, ReturnType<typeof deferred<string | null>>>();
    queue.syncDesired(
      [
        { id: "a", priority: 0 },
        { id: "b", priority: 1 },
        { id: "c", priority: 2 },
      ],
      (id) => {
        const d = deferred<string | null>();
        pending.set(id, d);
        return d.promise;
      },
    );

    pending.get("a")!.resolve("asset://a.jpg");
    pending.get("b")!.resolve("asset://b.jpg");
    pending.get("c")!.resolve("asset://c.jpg");
    await Promise.resolve();
    await Promise.resolve();

    // Not flushed yet before timer fires.
    expect(batches).toEqual([]);
    vi.advanceTimersByTime(80);
    expect(batches).toEqual([
      {
        a: "asset://a.jpg",
        b: "asset://b.jpg",
        c: "asset://c.jpg",
      },
    ]);
    queue.destroy();
  });
});
