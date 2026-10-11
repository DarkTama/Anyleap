import { describe, it, expect } from "vitest";
import { isNewer } from "./updateCheck";

describe("updateCheck", () => {
  describe("isNewer", () => {
    it("returns true when major, minor, or patch is strictly higher", () => {
      expect(isNewer("1.0.0", "0.9.6")).toBe(true);
      expect(isNewer("0.10.0", "0.9.6")).toBe(true);
      expect(isNewer("0.9.7", "0.9.6")).toBe(true);
    });

    it("returns false when latest is identical or older", () => {
      expect(isNewer("0.9.6", "0.9.6")).toBe(false);
      expect(isNewer("0.9.5", "0.9.6")).toBe(false);
      expect(isNewer("0.8.9", "0.9.0")).toBe(false);
      expect(isNewer("0.9.6", "1.0.0")).toBe(false);
    });

    it("handles 'v' prefix and prerelease tags cleanly", () => {
      expect(isNewer("v1.0.0", "0.9.6")).toBe(true);
      expect(isNewer("v0.9.6", "0.9.6")).toBe(false);
      expect(isNewer("v1.0.0-beta.1", "0.9.6")).toBe(true);
    });

    it("recognizes stable release as newer than prerelease with identical core version", () => {
      expect(isNewer("0.9.6", "0.9.6-beta")).toBe(true);
      expect(isNewer("0.9.6-beta", "0.9.6")).toBe(false);
    });
  });
});
