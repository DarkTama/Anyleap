export const INPUT_BLOCKED_KEY_MS = 30_000;
export const INPUT_BLOCKED_GLOBAL_MS = 10_000;

export function inputBlockedMessage(reason: string | undefined, proc: string): string {
  return reason === "secure_desktop"
    ? "Linked PC is showing a secure screen (UAC prompt / lock screen). It can't be controlled remotely."
    : `Linked PC: input blocked by an administrator window (${proc}). Run AnyLeap as administrator on the Linked PC to control admin apps. UAC prompts can't be controlled remotely.`;
}

/** Throttle for native notifications: 1 per 30 s per reason+process, 1 per 10 s overall. */
export function createInputBlockedThrottle(now: () => number = Date.now) {
  const lastByKey = new Map<string, number>();
  let lastAny: number | null = null;
  return function shouldNotify(reason: string | undefined, proc: string): boolean {
    const t = now();
    const key = `${reason ?? ""}|${proc}`;
    const k = lastByKey.get(key);
    if (lastAny !== null && t - lastAny < INPUT_BLOCKED_GLOBAL_MS) return false;
    if (k !== undefined && t - k < INPUT_BLOCKED_KEY_MS) return false;
    lastByKey.set(key, t);
    lastAny = t;
    return true;
  };
}
