import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createEmbedResizeScheduler, shouldSkipEmbedResize } from "./embedResize";

const r = (w: number, h: number) => ({ w, h, x: 0, y: 0 });

describe("embedResize", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("skips minimized and empty", () => {
    expect(shouldSkipEmbedResize(true, r(500, 500))).toBe(true);
    expect(shouldSkipEmbedResize(false, r(0, 0))).toBe(true);
    expect(shouldSkipEmbedResize(false, r(500, 0))).toBe(true);
    expect(shouldSkipEmbedResize(false, r(500, 400))).toBe(false);
  });

  it("debounces bursts to one trailing call", () => {
    const send = vi.fn();
    const s = createEmbedResizeScheduler(send, () => false);
    s.push(r(100, 100));
    vi.advanceTimersByTime(50);
    s.push(r(200, 200));
    vi.advanceTimersByTime(50);
    s.push(r(300, 300));
    expect(send).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith(r(300, 300));
  });

  it("does not send while minimized or 0x0", () => {
    const send = vi.fn();
    let min = true;
    const s = createEmbedResizeScheduler(send, () => min);
    s.push(r(500, 500));
    vi.advanceTimersByTime(150);
    expect(send).not.toHaveBeenCalled();
    min = false;
    s.push(r(0, 0));
    vi.advanceTimersByTime(150);
    expect(send).not.toHaveBeenCalled();
  });
});
