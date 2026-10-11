import { getVersion } from "@tauri-apps/api/app";
import { fetch } from "@tauri-apps/plugin-http";
import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

const REPO = "DarkTama/Anyleap";
const LATEST_API = `https://api.github.com/repos/${REPO}/releases/latest`;
const RELEASES_PAGE = `https://github.com/${REPO}/releases`;

interface ParsedSemver {
  major: number;
  minor: number;
  patch: number;
  prerelease: string | null;
}

/** "v1.2.3" / "1.2.3-beta" -> ParsedSemver. */
function parseSemver(v: string): ParsedSemver {
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

/** Launch-time update check. Never throws; silent on any failure. */
export async function checkForUpdates(): Promise<void> {
  try {
    const current = await getVersion();
    const res = await fetch(LATEST_API, {
      method: "GET",
      headers: {
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
      },
    });
    if (!res.ok) return;
    const data = (await res.json()) as { tag_name?: string };
    const tag = data.tag_name;
    if (!tag || !isNewer(tag, current)) return;

    const yes = await ask(
      `AnyLeap ${tag.replace(/^v/i, "")} is available (you have ${current}). ` +
        `Open the releases page to download it?`,
      {
        title: "Update available",
        kind: "info",
        okLabel: "Open releases",
        cancelLabel: "Later",
      },
    );
    if (yes) await openUrl(RELEASES_PAGE);
  } catch {
    // offline / rate-limited / parse error — silent by design
  }
}
