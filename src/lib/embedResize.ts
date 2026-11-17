export interface EmbedRect {
  w: number;
  h: number;
  x: number;
  y: number;
}

/** True when a resize must be skipped (minimized wrapper or empty container). */
export function shouldSkipEmbedResize(minimized: boolean, rect: EmbedRect): boolean {
  return minimized || rect.w <= 0 || rect.h <= 0;
}

/**
 * Trailing debounce for embedded child resizes. Bursts (drag, minimize/restore)
 * collapse into one call after `delayMs`; skipped rects are never sent.
 */
export function createEmbedResizeScheduler(
  send: (rect: EmbedRect) => void,
  isMinimized: () => boolean,
  delayMs = 100,
) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending: EmbedRect | null = null;
  return {
    push(rect: EmbedRect) {
      pending = rect;
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        const r = pending;
        pending = null;
        if (r && !shouldSkipEmbedResize(isMinimized(), r)) send(r);
      }, delayMs);
    },
    cancel() {
      if (timer) clearTimeout(timer);
      timer = null;
      pending = null;
    },
  };
}
