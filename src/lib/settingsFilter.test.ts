import { describe, expect, it } from "vitest";
import { filterSettings } from "./settingsFilter";
import { REGISTRY, SECTIONS } from "@/components/settings/registry";

describe("filterSettings", () => {
  it("empty query matches every entry and is inactive", () => {
    const r = filterSettings("  ");
    expect(r.active).toBe(false);
    expect(r.total).toBe(REGISTRY.length);
    expect(r.sections).toEqual(SECTIONS.map((s) => s.id));
    expect(r.advancedOpen).toBe(false);
  });

  it("matches labels case-insensitively", () => {
    const r = filterSettings("SHARE COPIED");
    expect([...r.matches].sort()).toEqual(["K-06", "K-07"]);
    expect(r.sections).toEqual(["clipboard"]);
  });

  it("matches helper text", () => {
    expect(filterSettings("prevents echo").matches.has("T-07")).toBe(true);
  });

  it("matches keywords: sound and mute find the audio switches", () => {
    const r = filterSettings("mute");
    expect(r.matches.has("M-10")).toBe(true);
    expect(r.matches.has("T-07")).toBe(true);
    expect(filterSettings("sound").counts.audio).toBeGreaterThan(0);
  });

  it("reports no match for gibberish", () => {
    const r = filterSettings("zzzqqq");
    expect(r.total).toBe(0);
    expect(r.sections).toEqual([]);
  });

  it("counts per section and auto-expands Advanced when it matches", () => {
    const r = filterSettings("large files");
    expect(r.counts.advanced).toBe(2);
    expect(r.advancedOpen).toBe(true);
    expect(filterSettings("battery").advancedOpen).toBe(false);
  });

  it("every control id has exactly one registry entry", () => {
    const ids = REGISTRY.map((e) => e.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ["K-01", "K-02", "T-12", "C-03", "M-29", "F-01", "F-02", "F-03"]) {
      expect(ids).toContain(id);
    }
    expect(ids).not.toContain("K-08");
    expect(ids).not.toContain("M-13");
  });

  it("matches F-01..F-03 transfer folder settings by label, helper, and keywords", () => {
    expect(filterSettings("folder").matches.has("F-01")).toBe(true);
    expect(filterSettings("prompt").matches.has("F-02")).toBe(true);
    expect(filterSettings("sdcard").matches.has("F-03")).toBe(true);
    expect(filterSettings("upload").matches.has("F-03")).toBe(true);
  });
});
