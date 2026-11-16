const DEFAULT_DEDUPE_WINDOW_MS = 5_000;

export function formatAdbError(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err ?? "");
  const cleaned = raw.replace(/^Error:\s*/i, "").trim();
  if (!cleaned) {
    return "ADB failed to respond. Click Refresh to try again.";
  }
  if (/click refresh|try refresh/i.test(cleaned)) {
    return cleaned;
  }
  if (/5037/i.test(cleaned)) {
    return `${cleaned} Close any conflicting ADB tool using port 5037 and click Refresh.`;
  }
  if (/\badb\b|daemon|timed out|fork-server/i.test(cleaned)) {
    return `${cleaned} Click Refresh to retry.`;
  }
  return cleaned;
}

export function normalizeAdbErrorKey(message: string): string {
  return message.trim().replace(/\s+/g, " ").toLowerCase();
}

export interface ErrorDeduper {
  shouldSurface: (message: string, now?: number) => boolean;
  reset: () => void;
}

export function createErrorDeduper(windowMs = DEFAULT_DEDUPE_WINDOW_MS): ErrorDeduper {
  const seen = new Map<string, number>();

  return {
    shouldSurface(message: string, now = Date.now()): boolean {
      const key = normalizeAdbErrorKey(message);
      if (!key) return false;
      const last = seen.get(key);
      if (last !== undefined && now - last < windowMs) {
        return false;
      }
      seen.set(key, now);
      return true;
    },
    reset(): void {
      seen.clear();
    },
  };
}

export const adbErrorDeduper = createErrorDeduper();

export function surfaceAdbError(
  err: unknown,
  setError: (msg: string | null) => void,
  deduper: ErrorDeduper = adbErrorDeduper,
  now?: number,
): string | null {
  const formatted = formatAdbError(err);
  if (!deduper.shouldSurface(formatted, now)) {
    return null;
  }
  setError(formatted);
  return formatted;
}
