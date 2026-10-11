import { useAppUpdateStore } from "@/stores/appUpdateStore";

interface ParsedSemver {
  major: number;
  minor: number;
  patch: number;
  prerelease: string | null;
}

/** "v1.2.3" / "1.2.3-beta" -> ParsedSemver. */
export function parseSemver(v: string): ParsedSemver {
  const clean = v.replace(/^v/i, "").trim();
  const [corePart, ...preParts] = clean.split("-");
  const core = corePart.split("+")[0];
  const parts = core.split(".").map((n) => parseInt(n, 10) || 0);
  const prerelease = preParts.length > 0 ? preParts.join("-").split("+")[0] : null;
  return {
    major: parts[0] ?? 0,
    minor: parts[1] ?? 0,
    patch: parts[2] ?? 0,
    prerelease,
  };
}

/** True if `latest` is strictly newer than `current`. */
export function isNewer(latest: string, current: string): boolean {
  const l = parseSemver(latest);
  const c = parseSemver(current);

  const lCore = [l.major, l.minor, l.patch];
  const cCore = [c.major, c.minor, c.patch];

  for (let i = 0; i < 3; i++) {
    if (lCore[i] > cCore[i]) return true;
    if (lCore[i] < cCore[i]) return false;
  }

  // Precedence rule (SemVer 2.0.0): normal version has higher precedence than prerelease
  if (l.prerelease === null && c.prerelease !== null) return true;
  if (l.prerelease !== null && c.prerelease === null) return false;

  return false;
}

/** Launch-time update check forwarding to Zustand appUpdateStore. */
export async function checkForUpdates(): Promise<void> {
  try {
    await useAppUpdateStore.getState().checkForUpdates(false);
  } catch {
    // silent failure
  }
}
