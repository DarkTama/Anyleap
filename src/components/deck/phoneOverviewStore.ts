import { useEffect } from "react";
import type { BatteryInfo } from "@/lib/types";
import { onPhoneOverview } from "@/services/phoneHubService";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import type { PhoneOverview } from "@/types/phone";

/**
 * Subscribes to `phone-overview` backend events while the Devices tab is mounted,
 * storing updates in the shared Phone Hub store.
 */
export function usePhoneOverviewListener(): void {
  const applyOverview = usePhoneHubStore((s) => s.applyOverview);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    void onPhoneOverview((payload) => {
      if (!active || !payload?.serial || !payload?.overview) return;
      applyOverview(payload.serial, payload.overview);
    })
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      })
      .catch(() => {
        // Non-Tauri test/browser environments do not have event listeners.
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [applyOverview]);
}

/**
 * Resolves battery chip data from `phone-overview` when available, otherwise falls back
 * to the existing `BatteryInfo` from `useAppStore.batteryMap`.
 */
export function resolveBatteryChipData(
  overview?: PhoneOverview | null,
  fallbackBattery?: BatteryInfo | null,
): { level: number | null; charging: boolean } | null {
  if (overview?.battery && overview.battery.level !== null && overview.battery.level !== undefined) {
    return {
      level: overview.battery.level,
      charging: Boolean(overview.battery.charging),
    };
  }
  if (fallbackBattery && fallbackBattery.level !== null && fallbackBattery.level !== undefined) {
    return {
      level: fallbackBattery.level,
      charging: Boolean(fallbackBattery.charging),
    };
  }
  return null;
}
