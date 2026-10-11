import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  parseSemver,
  isNewerVersion,
  calculateVersionDistance,
  fetchReleasesList,
  type ReleaseItem,
} from "../updateChecker";

describe("updateChecker", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("parses valid semver versions", () => {
    expect(parseSemver("0.9.6")).toEqual([0, 9, 6]);
    expect(parseSemver("v1.2.3")).toEqual([1, 2, 3]);
    expect(parseSemver("v0.10.2")).toEqual([0, 10, 2]);
    expect(parseSemver("invalid")).toBeNull();
  });

  it("correctly identifies newer versions", () => {
    expect(isNewerVersion("0.9.7", "0.9.6")).toBe(true);
    expect(isNewerVersion("0.10.0", "0.9.6")).toBe(true);
    expect(isNewerVersion("1.0.0", "0.9.6")).toBe(true);
    expect(isNewerVersion("0.9.6", "0.9.6")).toBe(false);
    expect(isNewerVersion("0.9.5", "0.9.6")).toBe(false);
    expect(isNewerVersion("v0.9.7", "0.9.6")).toBe(true);
  });

  it("calculates version distance and uninstalled releases", () => {
    const releases: ReleaseItem[] = [
      { id: 3, tagName: "v0.9.8", name: "v0.9.8", publishedAt: "2026-10-10", body: "Fix B", htmlUrl: "", isPrerelease: false },
      { id: 2, tagName: "v0.9.7", name: "v0.9.7", publishedAt: "2026-10-09", body: "Fix A", htmlUrl: "", isPrerelease: false },
      { id: 1, tagName: "v0.9.6", name: "v0.9.6", publishedAt: "2026-10-08", body: "Initial", htmlUrl: "", isPrerelease: false },
    ];
    const dist = calculateVersionDistance(releases, "0.9.6");
    expect(dist.isLatest).toBe(false);
    expect(dist.behindCount).toBe(2);
    expect(dist.latestVersion).toBe("0.9.8");
    expect(dist.uninstalledReleases).toHaveLength(2);
    expect(dist.uninstalledReleases[0].tagName).toBe("v0.9.8");
    expect(dist.uninstalledReleases[1].tagName).toBe("v0.9.7");
  });

  it("returns latest when already on top version", () => {
    const releases: ReleaseItem[] = [
      { id: 1, tagName: "v0.9.6", name: "v0.9.6", publishedAt: "2026-10-08", body: "Initial", htmlUrl: "", isPrerelease: false },
    ];
    const dist = calculateVersionDistance(releases, "0.9.6");
    expect(dist.isLatest).toBe(true);
    expect(dist.behindCount).toBe(0);
    expect(dist.uninstalledReleases).toHaveLength(0);
  });

  it("caches GitHub releases API response in localStorage", async () => {
    const mockReleases = [
      { id: 10, tag_name: "v0.9.7", name: "v0.9.7 Release", published_at: "2026-10-11", body: "Notes", html_url: "https://example.com", prerelease: false, draft: false }
    ];

    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => mockReleases,
    });
    vi.stubGlobal("fetch", fetchMock);

    const first = await fetchReleasesList(true);
    expect(first).toHaveLength(1);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    // Second call without force should hit localStorage cache
    const second = await fetchReleasesList(false);
    expect(second).toHaveLength(1);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
