import { describe, expect, it } from "vitest";
import { nextBatteryAlert } from "./batteryAlerts";

describe("nextBatteryAlert", () => {
  it("fires low once when crossing to <= 20% while not charging", () => {
    expect(
      nextBatteryAlert({ level: 21, charging: false }, { level: 20, charging: false }),
    ).toBe("low");

    // Stays in low band -> does not repeat
    expect(
      nextBatteryAlert({ level: 20, charging: false }, { level: 19, charging: false }),
    ).toBeNull();
  });

  it("re-arms low alert after charging or rising above 20%", () => {
    // Plugging in leaves low band
    expect(
      nextBatteryAlert({ level: 19, charging: false }, { level: 19, charging: true }),
    ).toBeNull();

    // Unplugging at 19% re-triggers low alert
    expect(
      nextBatteryAlert({ level: 19, charging: true }, { level: 19, charging: false }),
    ).toBe("low");

    // Charging above 20% then dropping back to 20% re-triggers
    expect(
      nextBatteryAlert({ level: 25, charging: false }, { level: 20, charging: false }),
    ).toBe("low");
  });

  it("fires full once when reaching 100% while charging and re-arms on leave", () => {
    expect(
      nextBatteryAlert({ level: 99, charging: true }, { level: 100, charging: true }),
    ).toBe("full");

    // Stays at 100% charging -> does not repeat
    expect(
      nextBatteryAlert({ level: 100, charging: true }, { level: 100, charging: true }),
    ).toBeNull();

    // Unplugs at 100% -> leaves band, no alert
    expect(
      nextBatteryAlert({ level: 100, charging: true }, { level: 100, charging: false }),
    ).toBeNull();

    // Plugs back in at 100% -> re-armed, fires full
    expect(
      nextBatteryAlert({ level: 100, charging: false }, { level: 100, charging: true }),
    ).toBe("full");
  });

  it("handles null prev and null level safely", () => {
    expect(nextBatteryAlert(null, { level: null, charging: false })).toBeNull();
    expect(nextBatteryAlert(null, { level: 15, charging: false })).toBe("low");
    expect(nextBatteryAlert(null, { level: 100, charging: true })).toBe("full");
    expect(nextBatteryAlert(null, { level: 50, charging: false })).toBeNull();
  });
});
