import { describe, it, expect } from "vitest";
import { KEYCODE } from "./keycodes";

describe("keycodes", () => {
  it("maps Android KeyEvent constants to correct official integer values", () => {
    expect(KEYCODE.HOME).toBe(3);
    expect(KEYCODE.BACK).toBe(4);
    expect(KEYCODE.VOLUME_UP).toBe(24);
    expect(KEYCODE.VOLUME_DOWN).toBe(25);
    expect(KEYCODE.POWER).toBe(26);
    expect(KEYCODE.SCREENSHOT).toBe(120);
    expect(KEYCODE.RECENTS).toBe(187);
    expect(KEYCODE.SLEEP).toBe(223);
    expect(KEYCODE.WAKEUP).toBe(224);
  });

  it("contains unique positive integer keycodes", () => {
    const values = Object.values(KEYCODE);
    const uniqueValues = new Set(values);

    expect(uniqueValues.size).toBe(values.length);
    for (const val of values) {
      expect(Number.isInteger(val)).toBe(true);
      expect(val).toBeGreaterThan(0);
    }
  });
});
