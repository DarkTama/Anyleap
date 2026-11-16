import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { BatteryChip, StatusChip, TransportChip } from "../DeviceConditionChips";
import type { PhoneOverview } from "@/types/phone";
import { resolveBatteryChipData } from "../../phoneOverviewStore";

const SAMPLE_OVERVIEW: PhoneOverview = {
  model: "Pixel 8",
  brand: "Google",
  androidVersion: "15",
  securityPatch: "2026-10-05",
  battery: { level: 91, charging: true, tempC: 31, health: "good" },
  storage: null,
  ram: null,
  wifi: null,
  cellular: null,
  screen: "on",
  uptimeSecs: 1200,
  updatedAtMs: 1700000000000,
};

describe("DeviceConditionChips & resolveBatteryChipData", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it("prefers phone-overview battery over fallback batteryMap", () => {
    expect(resolveBatteryChipData(SAMPLE_OVERVIEW, { level: 50, charging: false })).toEqual({
      level: 91,
      charging: true,
    });
    expect(resolveBatteryChipData(null, { level: 50, charging: false })).toEqual({
      level: 50,
      charging: false,
    });
    expect(resolveBatteryChipData({ ...SAMPLE_OVERVIEW, battery: null }, null)).toBeNull();
  });

  it("renders USB, Wireless, battery %, unknown —, and status chips", () => {
    act(() => {
      root.render(
        <div>
          <TransportChip transport="usb" />
          <TransportChip transport="wifi" />
          <BatteryChip battery={{ level: 18, charging: false }} showUnknown={true} />
          <BatteryChip battery={null} showUnknown={true} />
          <StatusChip status="connected" />
        </div>,
      );
    });
    expect(container.textContent).toContain("USB");
    expect(container.textContent).toContain("Wireless");
    expect(container.textContent).toContain("18%");
    expect(container.textContent).toContain("—");
    expect(container.textContent).toContain("Connected");
  });
});
