import { getVersion } from "@tauri-apps/api/app";
import { fetch } from "@tauri-apps/plugin-http";
import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

const REPO = "DarkTama/Anyleap";
const LATEST_API = `https://api.github.com/repos/${REPO}/releases/latest`;
const RELEASES_PAGE = `https://github.com/${REPO}/releases`;

/** "v1.2.3" / "1.2.3-beta" -> [major, minor, patch] (pre-release suffix ignored). */
function parseSemver(v: string): [number, number, number] {
  const core = v.replace(/^v/i, "").split(/[-+]/)[0];
  const parts = core.split(".").map((n) => parseInt(n, 10) || 0);
  return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0];
}

/** True if `latest` is strictly newer than `current`. */
export function isNewer(latest: string, current: string): boolean {
  const l = parseSemver(latest);
  const c = parseSemver(current);
  for (let i = 0; i < 3; i++) {
    if (l[i] > c[i]) return true;
    if (l[i] < c[i]) return false;
  }
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
