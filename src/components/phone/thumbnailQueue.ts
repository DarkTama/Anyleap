export interface ThumbnailQueueEntry {
  id: string;
  priority: number;
}

export type ThumbnailFetcher = (id: string) => Promise<string | null>;

interface PendingItem {
  id: string;
  priority: number;
  seq: number;
  fetcher: ThumbnailFetcher;
}

export interface ThumbnailQueueOptions {
  maxConcurrent?: number;
  batchIntervalMs?: number;
  onBatch: (updates: Record<string, string>) => void;
}

/**
 * Bounded-concurrency, priority-ordered thumbnail request queue.
 * - Caps concurrent device fetches (`maxConcurrent`, default 4).
 * - Prioritizes visible rows ahead of overscan rows.
 * - Drops queued requests for tiles that leave the viewport before starting.
 * - Ignores late results for unmounted/off-screen tiles or stale generations.
 * - Batches state updates (`batchIntervalMs`) to avoid re-rendering per tile.
 */
export class ThumbnailQueue {
  private readonly maxConcurrent: number;
  private readonly batchIntervalMs: number;
  private readonly onBatch: (updates: Record<string, string>) => void;

  private generation = 0;
  private seqCounter = 0;
  private desiredIds = new Set<string>();
  private settledIds = new Set<string>();
  private inFlight = new Set<string>(); // `${generation}:${id}`
  private pending: PendingItem[] = [];
  private pendingBatch: Record<string, string> = {};
  private flushTimer: ReturnType<typeof setTimeout> | null = null;
  private destroyed = false;

  constructor(options: ThumbnailQueueOptions) {
    this.maxConcurrent = Math.max(1, options.maxConcurrent ?? 4);
    this.batchIntervalMs = Math.max(0, options.batchIntervalMs ?? 80);
    this.onBatch = options.onBatch;
  }

  syncDesired(entries: ThumbnailQueueEntry[], fetcher: ThumbnailFetcher): void {
    if (this.destroyed) return;

    const nextDesired = new Set<string>();
    const priorityById = new Map<string, number>();
    for (const entry of entries) {
      nextDesired.add(entry.id);
      const prev = priorityById.get(entry.id);
      if (prev === undefined || entry.priority < prev) {
        priorityById.set(entry.id, entry.priority);
      }
    }
    this.desiredIds = nextDesired;

    // Drop queued items that left the viewport (+overscan) before starting,
    // and refresh priority for items still desired.
    this.pending = this.pending.filter((item) => {
      const p = priorityById.get(item.id);
      if (p === undefined) return false;
      item.priority = p;
      item.fetcher = fetcher;
      return true;
    });

    const pendingIds = new Set(this.pending.map((item) => item.id));
    const gen = this.generation;

    for (const [id, priority] of priorityById.entries()) {
      if (
        this.settledIds.has(id) ||
        this.inFlight.has(this.flightKey(gen, id)) ||
        pendingIds.has(id)
      ) {
        continue;
      }
      this.pending.push({
        id,
        priority,
        seq: this.seqCounter++,
        fetcher,
      });
      pendingIds.add(id);
    }

    this.pending.sort((a, b) => a.priority - b.priority || a.seq - b.seq);
    this.pump();
  }

  reset(): void {
    this.generation += 1;
    this.desiredIds.clear();
    this.settledIds.clear();
    this.pending = [];
    this.pendingBatch = {};
    this.clearFlushTimer();
  }

  destroy(): void {
    this.destroyed = true;
    this.reset();
  }

  flushNow(): void {
    this.clearFlushTimer();
    if (this.destroyed) {
      this.pendingBatch = {};
      return;
    }
    const keys = Object.keys(this.pendingBatch);
    if (keys.length === 0) return;
    const batch = this.pendingBatch;
    this.pendingBatch = {};
    this.onBatch(batch);
  }

  getStats(): {
    inFlightCount: number;
    queuedIds: string[];
    desiredCount: number;
    generation: number;
  } {
    return {
      inFlightCount: this.inFlight.size,
      queuedIds: this.pending.map((p) => p.id),
      desiredCount: this.desiredIds.size,
      generation: this.generation,
    };
  }

  private flightKey(gen: number, id: string): string {
    return `${gen}:${id}`;
  }

  private clearFlushTimer(): void {
    if (this.flushTimer !== null) {
      clearTimeout(this.flushTimer);
      this.flushTimer = null;
    }
  }

  private scheduleFlush(): void {
    if (this.destroyed || this.flushTimer !== null) return;
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null;
      this.flushNow();
    }, this.batchIntervalMs);
  }

  private pump(): void {
    while (!this.destroyed && this.inFlight.size < this.maxConcurrent && this.pending.length > 0) {
      const next = this.pending.shift()!;
      if (!this.desiredIds.has(next.id) || this.settledIds.has(next.id)) {
        continue;
      }
      const gen = this.generation;
      const key = this.flightKey(gen, next.id);
      if (this.inFlight.has(key)) {
        continue;
      }
      this.inFlight.add(key);

      let callPromise: Promise<string | null>;
      try {
        callPromise = Promise.resolve(next.fetcher(next.id));
      } catch (err) {
        callPromise = Promise.reject(err);
      }

      callPromise
        .then(
          (src) => {
            this.inFlight.delete(key);
            if (this.destroyed || gen !== this.generation || !this.desiredIds.has(next.id)) {
              // Ignore late results for unmounted/off-screen tiles or stale generations.
              return;
            }
            this.settledIds.add(next.id);
            if (src) {
              this.pendingBatch[next.id] = src;
              this.scheduleFlush();
            }
          },
          () => {
            this.inFlight.delete(key);
            if (this.destroyed || gen !== this.generation || !this.desiredIds.has(next.id)) {
              return;
            }
            this.settledIds.add(next.id);
          },
        )
        .finally(() => {
          if (!this.destroyed) {
            this.pump();
          }
        });
    }
  }
}
