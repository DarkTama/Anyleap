import { describe, it, expect, vi } from "vitest";
import { createErrorDeduper, formatAdbError, surfaceAdbError } from "./adbError";

describe("adbError", () => {
  describe("formatAdbError", () => {
    it("strips leading Error: prefix and trims whitespace", () => {
      expect(formatAdbError(new Error("  adb devices failed: offline  "))).toBe(
        "adb devices failed: offline Click Refresh to retry.",
      );
    });

    it("provides a friendly fallback when error is empty", () => {
      expect(formatAdbError("")).toBe("ADB failed to respond. Click Refresh to try again.");
      expect(formatAdbError(null)).toBe("ADB failed to respond. Click Refresh to try again.");
    });

    it("adds port 5037 guidance when port 5037 is mentioned without existing guidance", () => {
      expect(formatAdbError("cannot bind to 127.0.0.1:5037")).toBe(
        "cannot bind to 127.0.0.1:5037 Close any conflicting ADB tool using port 5037 and click Refresh.",
      );
    });

    it("does not duplicate guidance already formatted by backend", () => {
      const backendMsg =
        "Could not start ADB server on port 5037. Close any conflicting ADB tool using port 5037 and click Refresh.";
      expect(formatAdbError(backendMsg)).toBe(backendMsg);
    });
  });

  describe("createErrorDeduper & surfaceAdbError", () => {
    it("surfaces identical or whitespace-variant errors only once inside the window", () => {
      const deduper = createErrorDeduper(5_000);
      const setError = vi.fn();

      const first = surfaceAdbError("adb devices failed: timeout", setError, deduper, 1_000);
      const dup = surfaceAdbError("  ADB   devices failed: timeout ", setError, deduper, 2_000);

      expect(first).toContain("adb devices failed: timeout");
      expect(dup).toBeNull();
      expect(setError).toHaveBeenCalledTimes(1);
    });

    it("allows surfacing again after windowMs elapses or after reset()", () => {
      const deduper = createErrorDeduper(5_000);
      const setError = vi.fn();

      expect(surfaceAdbError("adb daemon crashed", setError, deduper, 1_000)).not.toBeNull();
      expect(surfaceAdbError("adb daemon crashed", setError, deduper, 5_999)).toBeNull();
      expect(surfaceAdbError("adb daemon crashed", setError, deduper, 6_000)).not.toBeNull();

      deduper.reset();
      expect(surfaceAdbError("adb daemon crashed", setError, deduper, 6_100)).not.toBeNull();
      expect(setError).toHaveBeenCalledTimes(3);
    });

    it("surfaces distinct error messages independently", () => {
      const deduper = createErrorDeduper(5_000);
      const setError = vi.fn();

      expect(surfaceAdbError("adb daemon failed", setError, deduper, 1_000)).not.toBeNull();
      expect(surfaceAdbError("scrcpy binary missing", setError, deduper, 1_100)).not.toBeNull();
      expect(setError).toHaveBeenCalledTimes(2);
    });
  });
});
