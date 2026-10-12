import { describe, expect, it } from "vitest";
import { stripAssetsSection } from "./releaseNotes";

describe("stripAssetsSection", () => {
  it("removes the Assets section and keeps the rest", () => {
    const body = "## What's Changed\n\n- a\n\n### Assets\n- `x.exe`\n";
    expect(stripAssetsSection(body)).toBe("## What's Changed\n\n- a");
  });
  it("leaves bodies without Assets untouched", () => {
    expect(stripAssetsSection("- a")).toBe("- a");
  });
});
