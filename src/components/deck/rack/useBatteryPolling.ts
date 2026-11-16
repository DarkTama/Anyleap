import { useEffect, useRef } from "react";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { getBatteryInfo } from "@/lib/tauri";
import { nextBatteryAlert, type BatteryAlertKind } from "@/lib/batteryAlerts";
import type { BatteryInfo, DeviceInfo } from "@/lib/types";

async function notifyBattery(
  deviceName: string,
  kind: BatteryAlertKind,
  level: number,
): Promise<void> {
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      const perm = await requestPermission();
      granted = perm === "granted";
    }
    if (!granted) return;
    sendNotification({
      title:
        kind === "low"
          ? `${deviceName}: Battery low (${level}%)`
          : `${deviceName}: Battery full (100%)`,
      body:
        kind === "low"
          ? "Plug in your device to avoid disconnecting."
          : "Device is fully charged.",
    });
  } catch {
    // best-effort notification
  }
}

/**
 * Polls battery telemetry every 15 s for all online devices and fires
 * optional low/full battery desktop notifications on state transitions.
 */
export function useBatteryPolling(
  devices: DeviceInfo[],
  setBatteryInfo: (serial: string, info: BatteryInfo) => void,
  batteryAlerts: boolean,
): void {
  const prevBatteryRef = useRef<Record<string, BatteryInfo>>({});

  useEffect(() => {
    let cancelled = false;
    const fetchBatteries = async () => {
      for (const dev of devices) {
        if (dev.state === "device") {
          try {
            const info = await getBatteryInfo(dev.serial);
            if (!cancelled) {
              const prev = prevBatteryRef.current[dev.serial];
              prevBatteryRef.current[dev.serial] = info;
              if (batteryAlerts) {
                const alert = nextBatteryAlert(prev, info);
                if (alert) {
                  void notifyBattery(
                    dev.model || dev.product || dev.serial,
                    alert,
                    info.level ?? 0,
                  );
                }
              }
              setBatteryInfo(dev.serial, info);
            }
          } catch {
            // best-effort
          }
        }
      }
    };

    void fetchBatteries();
    const timer = setInterval(() => void fetchBatteries(), 15000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [devices, setBatteryInfo, batteryAlerts]);
}
