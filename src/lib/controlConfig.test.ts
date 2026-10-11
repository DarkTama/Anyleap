import { describe, it, expect } from "vitest";
import {
  BUTTON_IDS,
  BUTTON_LABELS,
  CONTROL_SIZES,
  DEFAULT_CONTROL_CONFIG,
  DOCK_SIDES,
} from "./controlConfig";

describe("controlConfig", () => {
  it("has unique button IDs", () => {
    const unique = new Set(BUTTON_IDS);
    expect(unique.size).toBe(BUTTON_IDS.length);
  });

  it("provides a non-empty human-readable label for every button ID", () => {
    for (const id of BUTTON_IDS) {
      expect(BUTTON_LABELS[id]).toBeDefined();
      expect(BUTTON_LABELS[id].trim().length).toBeGreaterThan(0);
    }
  });

  it("configures default buttons covering every button ID", () => {
    for (const id of BUTTON_IDS) {
      expect(typeof DEFAULT_CONTROL_CONFIG.buttons[id]).toBe("boolean");
    }
  });

  it("uses valid enum values in DEFAULT_CONTROL_CONFIG", () => {
    expect(DOCK_SIDES).toContain(DEFAULT_CONTROL_CONFIG.dock);
    expect(CONTROL_SIZES).toContain(DEFAULT_CONTROL_CONFIG.size);
    expect(DEFAULT_CONTROL_CONFIG.collapsed).toBe(false);
  });
});
