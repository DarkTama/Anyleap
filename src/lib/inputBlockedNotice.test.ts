import { describe, expect, it } from "vitest";
import { createInputBlockedThrottle } from "./inputBlockedNotice";

describe("input blocked throttle", () => {
  it("throttles per key and globally", () => {
    let t = 1000;
    const ok = createInputBlockedThrottle(() => t);
    expect(ok("uipi", "a.exe")).toBe(true);
    t += 2000;
    expect(ok("uipi", "b.exe")).toBe(false); // global 10s
    t += 9000; // 11s since first
    expect(ok("uipi", "b.exe")).toBe(true);
    t += 11000; // 22s since first; a.exe still within 30s
    expect(ok("uipi", "a.exe")).toBe(false);
    t += 9000; // 31s since a.exe
    expect(ok("uipi", "a.exe")).toBe(true);
  });
  it("blocked attempts do not extend windows", () => {
    let t = 0;
    const ok = createInputBlockedThrottle(() => t);
    expect(ok("secure_desktop", "x")).toBe(true);
    t = 5000;
    expect(ok("secure_desktop", "x")).toBe(false);
    t = 30_000;
    expect(ok("secure_desktop", "x")).toBe(true);
  });
});
