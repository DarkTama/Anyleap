/** SPEC-29 §7.4: one-time Flex warning about view-once media. */
import type { KeyValueStore } from "./settingsStore";

export const FLEX_SECURE_WARNING_KEY = "flexSecureWarningDismissed";
export const FLEX_SECURE_WARNING_TEXT =
  "Opening view-once media in Flex shows black and still marks it as seen.";

/**
 * True when the warning must show now. Marks it as seen in the same step, so it
 * returns only if the flag is cleared (per user, not per device).
 */
export async function takeFlexSecureWarning(store: KeyValueStore): Promise<boolean> {
  if ((await store.get<boolean>(FLEX_SECURE_WARNING_KEY)) === true) return false;
  await store.set(FLEX_SECURE_WARNING_KEY, true);
  await store.save();
  return true;
}
