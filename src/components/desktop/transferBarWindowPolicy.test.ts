import { describe, expect, it } from "vitest";
import { decideTransferBarWindowAction as d } from "./transferBarWindowPolicy";

describe("decideTransferBarWindowAction", () => {
  it("opens when content and no window", () => {
    expect(d({ hasContent: true, hadContent: false, windowExists: false })).toBe("open");
  });
  it("recreates a stale window left from an empty bar (regression: notice never re-shown)", () => {
    expect(d({ hasContent: true, hadContent: false, windowExists: true })).toBe("recreate");
  });
  it("reveals existing window when content continues", () => {
    expect(d({ hasContent: true, hadContent: true, windowExists: true })).toBe("reveal");
  });
  it("closes when emptied", () => {
    expect(d({ hasContent: false, hadContent: true, windowExists: true })).toBe("close");
  });
  it("does nothing when idle", () => {
    expect(d({ hasContent: false, hadContent: false, windowExists: true })).toBe("none");
  });
});
