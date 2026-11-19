import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createQrPairingSession } from "./qrPairingSession";
import type { MdnsService } from "./types";

const svc = (name: string, type = "_adb-tls-pairing._tcp"): MdnsService =>
  ({ name, serviceType: type, host: "1.2.3.4", port: 4000 }) as MdnsService;

let n = 0;
const generate = () => {
  n++;
  return { name: `adbqr-${n}`, password: `pw${n}`, payload: `p${n}` };
};

beforeEach(() => {
  vi.useFakeTimers();
  n = 0;
});
afterEach(() => vi.useRealTimers());

describe("qr pairing session", () => {
  it("shows no QR when not listening", async () => {
    const s = createQrPairingSession({ discover: async () => [], pair: vi.fn(), generate });
    expect(s.getState().challenge).toBeNull();
    s.start();
    expect(s.getState().challenge?.name).toBe("adbqr-1");
    s.cancel();
    expect(s.getState().challenge).toBeNull();
  });

  it("regenerates on expiry, keeps polling, then expires at cap", async () => {
    const discover = vi.fn(async () => []);
    const s = createQrPairingSession({
      discover,
      pair: vi.fn(),
      generate,
      listenWindowMs: 10_000,
      overallCapMs: 25_000,
    });
    s.start();
    await vi.advanceTimersByTimeAsync(10_500);
    expect(s.getState().challenge?.name).toBe("adbqr-2");
    const calls = discover.mock.calls.length;
    await vi.advanceTimersByTimeAsync(2000);
    expect(discover.mock.calls.length).toBeGreaterThan(calls);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(s.getState().phase).toBe("expired");
    expect(s.getState().challenge).toBeNull();
    s.cancel();
  });

  it("exact-name match pairs once; ignores adb-<guid>", async () => {
    const pair = vi.fn(async () => undefined);
    const onPaired = vi.fn();
    let found: MdnsService[] = [svc("adb-abcd")];
    const s = createQrPairingSession({ discover: async () => found, pair, generate, onPaired });
    s.start();
    await vi.advanceTimersByTimeAsync(3000);
    expect(pair).not.toHaveBeenCalled();
    found = [svc("adb-abcd"), svc("adbqr-1")];
    await vi.advanceTimersByTimeAsync(3000);
    expect(pair).toHaveBeenCalledTimes(1);
    expect(pair).toHaveBeenCalledWith("1.2.3.4", 4000, "pw1");
    expect(onPaired).toHaveBeenCalledTimes(1);
    expect(s.getState().phase).toBe("paired");
  });

  it("surfaces pair failure and retry makes a new challenge", async () => {
    const pair = vi.fn(async () => {
      throw new Error("bad code");
    });
    const s = createQrPairingSession({ discover: async () => [svc("adbqr-1")], pair, generate });
    s.start();
    await vi.advanceTimersByTimeAsync(1000);
    expect(s.getState().phase).toBe("failed");
    expect(s.getState().error).toContain("bad code");
    s.retry();
    expect(s.getState().challenge?.name).toBe("adbqr-2");
    s.cancel();
  });

  it("cancel stops polling", async () => {
    const discover = vi.fn(async () => []);
    const s = createQrPairingSession({ discover, pair: vi.fn(), generate });
    s.start();
    await vi.advanceTimersByTimeAsync(2000);
    s.cancel();
    const c = discover.mock.calls.length;
    await vi.advanceTimersByTimeAsync(5000);
    expect(discover.mock.calls.length).toBe(c);
  });

  it("previous challenge still pairs within grace with its own password", async () => {
    const pair = vi.fn(async () => undefined);
    let found: MdnsService[] = [];
    const s = createQrPairingSession({
      discover: async () => found,
      pair,
      generate,
      listenWindowMs: 10_000,
      graceMs: 60_000,
    });
    s.start();
    await vi.advanceTimersByTimeAsync(10_500);
    expect(s.getState().challenge?.name).toBe("adbqr-2");
    await vi.advanceTimersByTimeAsync(5000);
    found = [svc("adbqr-1")];
    await vi.advanceTimersByTimeAsync(2000);
    expect(pair).toHaveBeenCalledTimes(1);
    expect(pair).toHaveBeenCalledWith("1.2.3.4", 4000, "pw1");
    s.cancel();
  });

  it("challenge older than grace is ignored", async () => {
    const pair = vi.fn(async () => undefined);
    let found: MdnsService[] = [];
    const s = createQrPairingSession({
      discover: async () => found,
      pair,
      generate,
      listenWindowMs: 10_000,
      graceMs: 15_000,
      overallCapMs: 600_000,
    });
    s.start();
    await vi.advanceTimersByTimeAsync(40_000);
    found = [svc("adbqr-1")];
    await vi.advanceTimersByTimeAsync(3000);
    expect(pair).not.toHaveBeenCalled();
    s.cancel();
  });
});
