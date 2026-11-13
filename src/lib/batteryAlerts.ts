export interface BatterySnapshot {
  level: number | null;
  charging: boolean;
}

export type BatteryAlertKind = "low" | "full";

function inLowBand(s: BatterySnapshot): boolean {
  return s.level !== null && s.level <= 20 && !s.charging;
}

function inFullBand(s: BatterySnapshot): boolean {
  return s.level !== null && s.level >= 100 && s.charging;
}

/**
 * Determine whether a battery transition warrants a one-shot alert.
 * Re-arms automatically after the device leaves the band.
 */
export function nextBatteryAlert(
  prev: BatterySnapshot | null | undefined,
  current: BatterySnapshot,
): BatteryAlertKind | null {
  if (inLowBand(current) && (!prev || !inLowBand(prev))) {
    return "low";
  }
  if (inFullBand(current) && (!prev || !inFullBand(prev))) {
    return "full";
  }
  return null;
}
