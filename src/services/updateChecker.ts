import { getVersion } from "@tauri-apps/api/app";

export interface UpdateInfo {
  hasUpdate: boolean;
  currentVersion: string;
  latestVersion: string;
  releaseUrl: string;
}

export interface ReleaseItem {
  id: number;
  tagName: string;
  name: string;
  publishedAt: string;
  body: string;
  htmlUrl: string;
  isPrerelease: boolean;
}

export interface VersionDistanceResult {
  isLatest: boolean;
  behindCount: number;
  latestVersion: string;
  uninstalledReleases: ReleaseItem[];
}

const REPO = "DarkTama/Anyleap";
const GITHUB_API_LATEST = `https://api.github.com/repos/${REPO}/releases/latest`;
const GITHUB_API_RELEASES = `https://api.github.com/repos/${REPO}/releases?per_page=15`;
const CHECK_INTERVAL_MS = 30 * 60 * 1000; // 30 minutes

export function parseSemver(v: string): [number, number, number] | null {
  const clean = v.trim().replace(/^v/i, "");
  const match = clean.match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!match) return null;
  return [parseInt(match[1], 10), parseInt(match[2], 10), parseInt(match[3], 10)];
}

export function isNewerVersion(remote: string, current: string): boolean {
  const r = parseSemver(remote);
  const c = parseSemver(current);
  if (!r || !c) return false;
  if (r[0] > c[0]) return true;
  if (r[0] < c[0]) return false;
  if (r[1] > c[1]) return true;
  if (r[1] < c[1]) return false;
  return r[2] > c[2];
}

export async function getCurrentAppVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "0.9.6";
  }
}

export async function checkLatestRelease(
  force = false,
  currentVersion?: string
): Promise<UpdateInfo | null> {
  const appVersion = currentVersion ?? (await getCurrentAppVersion());
  const now = Date.now();
  const lastCheck = typeof localStorage !== "undefined" ? localStorage.getItem("anyleap_last_update_check") : null;

  if (!force && lastCheck) {
    const elapsed = now - parseInt(lastCheck, 10);
    if (elapsed < CHECK_INTERVAL_MS) {
      const cached = localStorage.getItem("anyleap_cached_update_info");
      if (cached) {
        try {
          const parsed = JSON.parse(cached) as UpdateInfo;
          if (parsed.currentVersion === appVersion) {
            parsed.hasUpdate = isNewerVersion(parsed.latestVersion, appVersion);
            return parsed;
          }
        } catch {
          // ignore cache parse errors
        }
      }
    }
  }

  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 5000);

    const res = await fetch(GITHUB_API_LATEST, {
      signal: controller.signal,
      headers: {
        Accept: "application/vnd.github.v3+json",
      },
    });
    clearTimeout(timer);

    if (!res.ok) return null;

    const data = await res.json();
    const latestVersion = (data.tag_name || "").trim().replace(/^v/i, "");
    const releaseUrl = data.html_url || `https://github.com/${REPO}/releases`;
    const hasUpdate = isNewerVersion(latestVersion, appVersion);

    const info: UpdateInfo = {
      hasUpdate,
      currentVersion: appVersion,
      latestVersion,
      releaseUrl,
    };

    if (typeof localStorage !== "undefined") {
      localStorage.setItem("anyleap_last_update_check", now.toString());
      localStorage.setItem("anyleap_cached_update_info", JSON.stringify(info));
    }

    return info;
  } catch {
    return null;
  }
}

export async function fetchReleasesList(force = false): Promise<ReleaseItem[]> {
  const now = Date.now();
  if (!force && typeof localStorage !== "undefined") {
    const lastCheck = localStorage.getItem("anyleap_last_releases_check");
    if (lastCheck) {
      const elapsed = now - parseInt(lastCheck, 10);
      if (elapsed < CHECK_INTERVAL_MS) {
        const cached = localStorage.getItem("anyleap_cached_releases");
        if (cached) {
          try {
            return JSON.parse(cached) as ReleaseItem[];
          } catch {
            // ignore cache parse errors
          }
        }
      }
    }
  }

  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 5000);

    const res = await fetch(GITHUB_API_RELEASES, {
      signal: controller.signal,
      headers: {
        Accept: "application/vnd.github.v3+json",
      },
    });
    clearTimeout(timer);

    if (!res.ok) return [];

    const data = await res.json();
    if (!Array.isArray(data)) return [];

    const releases: ReleaseItem[] = data
      .filter((item: any) => !item.draft)
      .map((item: any) => ({
        id: item.id,
        tagName: item.tag_name || "",
        name: item.name || item.tag_name || "",
        publishedAt: item.published_at || "",
        body: item.body || "",
        htmlUrl: item.html_url || "",
        isPrerelease: Boolean(item.prerelease),
      }));

    if (typeof localStorage !== "undefined") {
      localStorage.setItem("anyleap_last_releases_check", now.toString());
      localStorage.setItem("anyleap_cached_releases", JSON.stringify(releases));
    }

    return releases;
  } catch {
    return [];
  }
}

export function calculateVersionDistance(
  releases: ReleaseItem[],
  currentVersion: string
): VersionDistanceResult {
  const cleanCurrent = currentVersion.trim().replace(/^v/i, "");
  if (!releases || releases.length === 0) {
    return {
      isLatest: true,
      behindCount: 0,
      latestVersion: cleanCurrent,
      uninstalledReleases: [],
    };
  }

  const uninstalled = releases.filter((r) => isNewerVersion(r.tagName, cleanCurrent));

  return {
    isLatest: uninstalled.length === 0,
    behindCount: uninstalled.length,
    latestVersion: uninstalled.length > 0 ? uninstalled[0].tagName.replace(/^v/i, "") : cleanCurrent,
    uninstalledReleases: uninstalled,
  };
}
