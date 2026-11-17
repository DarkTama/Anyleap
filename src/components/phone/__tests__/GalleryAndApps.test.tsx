import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import type { Album, MediaItem, MediaPage, PhoneApp } from "@/types/phone";

const mockPhoneListAlbums = vi.fn<(...args: unknown[]) => Promise<Album[]>>(() =>
  Promise.resolve([
    { name: "Camera", photoCount: 70, videoCount: 2 },
    { name: "Screenshots", photoCount: 10, videoCount: 0 },
  ]),
);
const mockPhoneListMedia = vi.fn<(...args: unknown[]) => Promise<MediaPage>>();
const mockPhoneMediaThumbnail = vi.fn<(...args: unknown[]) => Promise<string | null>>(() =>
  Promise.resolve(null),
);
const mockPhoneOpenMedia = vi.fn((..._args: unknown[]) => Promise.resolve());
const mockPhoneListApps = vi.fn<(...args: unknown[]) => Promise<PhoneApp[]>>(() =>
  Promise.resolve([
    { package: "app.example.chat", label: "Chat", system: false },
    { package: "app.example.notes", label: "Notes", system: false },
    { package: "system.settings", label: "Settings", system: true },
  ]),
);
let pinnedState: string[] = [];
const mockPhoneGetPinnedApps = vi.fn<(...args: unknown[]) => Promise<string[]>>(() =>
  Promise.resolve([...pinnedState]),
);
const mockPhoneSetAppPinned = vi.fn(
  (_serial: unknown, pkg: unknown, pinned: unknown) => {
    pinnedState = pinnedState.filter((p) => p !== pkg);
    if (pinned) pinnedState.push(pkg as string);
    return Promise.resolve([...pinnedState]);
  },
);
const mockPhoneOpenApp = vi.fn((..._args: unknown[]) => Promise.resolve({}));
const mockPullDeviceFile = vi.fn((..._args: unknown[]) => Promise.resolve("C:\\Downloads\\item.jpg"));
const mockOpenDialog = vi.fn((..._args: unknown[]) => Promise.resolve("C:\\Downloads"));

vi.mock("@/services/phoneHubService", () => ({
  phoneListAlbums: (...args: unknown[]) => mockPhoneListAlbums(...args),
  phoneListMedia: (...args: unknown[]) => mockPhoneListMedia(...args),
  phoneMediaThumbnail: (...args: unknown[]) => mockPhoneMediaThumbnail(...args),
  phoneOpenMedia: (...args: unknown[]) => mockPhoneOpenMedia(...args),
  phoneListApps: (...args: unknown[]) => mockPhoneListApps(...args),
  phoneGetPinnedApps: (...args: unknown[]) => mockPhoneGetPinnedApps(...args),
  phoneSetAppPinned: (...args: unknown[]) =>
    mockPhoneSetAppPinned(...(args as [unknown, unknown, unknown])),
  phoneOpenApp: (...args: unknown[]) => mockPhoneOpenApp(...args),
}));

vi.mock("@/lib/tauri", () => ({
  pullDeviceFile: (...args: unknown[]) => mockPullDeviceFile(...args),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => mockOpenDialog(...args),
}));

vi.mock("@tauri-apps/api/path", () => ({
  downloadDir: vi.fn(() => Promise.resolve("C:\\Downloads")),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (p: string) => `asset://${p}`,
}));

import {
  GalleryView,
  GALLERY_PAGE_SIZE,
  MIN_TILE_SIZE,
  GALLERY_TILE_GAP,
  computeGalleryGridMetrics,
} from "../GalleryView";
import { AppsView } from "../AppsView";

function makeMediaItem(i: number, kind: "photo" | "video" = "photo"): MediaItem {
  return {
    id: `m-${i}`,
    kind,
    name: kind === "video" ? `VID_${i}.mp4` : `IMG_${i}.jpg`,
    album: "Camera",
    path: `/storage/emulated/0/DCIM/Camera/${kind === "video" ? `VID_${i}.mp4` : `IMG_${i}.jpg`}`,
    dateTakenMs: 1700000000000 - i * 1000,
    sizeBytes: 2_000_000,
    width: 1920,
    height: 1080,
    durationMs: kind === "video" ? 65_000 : null,
  };
}

describe("GalleryView & AppsView (SPEC-27b Task B4)", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    mockPhoneListAlbums.mockClear();
    mockPhoneListMedia.mockReset();
    mockPhoneMediaThumbnail.mockReset();
    mockPhoneMediaThumbnail.mockResolvedValue(null);
    mockPhoneOpenMedia.mockClear();
    mockPhoneListApps.mockClear();
    mockPhoneOpenApp.mockClear();
    mockPullDeviceFile.mockClear();
    mockOpenDialog.mockClear();
    usePhoneHubStore.getState().reset();
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
  });

  it("computes auto-fill gallery columns and square tile size to fill available width", () => {
    const narrow = computeGalleryGridMetrics(200);
    expect(narrow.cols).toBe(1);
    expect(narrow.tileSize).toBe(168);
    expect(narrow.rowStride).toBe(168 + GALLERY_TILE_GAP);

    // ~1270px window minus sidebar (~208px) => ~1062px container; 1030px inner width
    const desktop = computeGalleryGridMetrics(1062);
    expect(desktop.cols).toBe(7);
    expect(desktop.tileSize).toBeGreaterThanOrEqual(MIN_TILE_SIZE);
    const totalUsedWidth =
      desktop.cols * desktop.tileSize + (desktop.cols - 1) * GALLERY_TILE_GAP;
    expect(totalUsedWidth).toBeCloseTo(1062 - 32, 5);
    expect(desktop.rowStride).toBeCloseTo(desktop.tileSize + GALLERY_TILE_GAP, 5);

    // 800px container (5 cols) expands tiles to eliminate the ~96px gutter
    const medium = computeGalleryGridMetrics(800);
    expect(medium.cols).toBe(5);
    expect(medium.tileSize).toBeCloseTo((800 - 32 - 4 * GALLERY_TILE_GAP) / 5, 5);
  });

  it("pages gallery in chunks of 60 without duplicates, bounds DOM, and resets on filter change", async () => {
    const page1 = Array.from({ length: GALLERY_PAGE_SIZE }, (_, i) =>
      makeMediaItem(i, i === 0 ? "video" : "photo"),
    );
    const page2 = [
      makeMediaItem(59), // duplicate id to verify dedupe
      makeMediaItem(60),
      makeMediaItem(61),
    ];

    mockPhoneListMedia
      .mockResolvedValueOnce({ items: page1, total: 62 })
      .mockResolvedValueOnce({ items: page2, total: 62 })
      .mockResolvedValueOnce({ items: [makeMediaItem(0, "video")], total: 1 });

    await act(async () => {
      root.render(<GalleryView serial="serial-1" />);
    });

    expect(mockPhoneListMedia).toHaveBeenCalledWith(
      "serial-1",
      "all",
      null,
      0,
      60,
    );
    expect(container.textContent).toContain("62 items");

    // DOM is windowed (fewer than 60 tiles rendered in 800x600 viewport)
    const renderedTiles = container.querySelectorAll('button[aria-label^="Photo "], button[aria-label^="Video "]');
    expect(renderedTiles.length).toBeGreaterThan(0);
    expect(renderedTiles.length).toBeLessThan(60);

    // Click Load more
    const loadMoreBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Load more"),
    );
    await act(async () => {
      loadMoreBtn?.click();
    });

    expect(mockPhoneListMedia).toHaveBeenCalledWith(
      "serial-1",
      "all",
      null,
      60,
      60,
    );
    expect(container.textContent).toContain("End of album · 62 items");

    // Switch filter to Videos -> resets paging to offset 0
    const videosBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent === "Videos",
    );
    await act(async () => {
      videosBtn?.click();
    });

    expect(mockPhoneListMedia).toHaveBeenLastCalledWith(
      "serial-1",
      "video",
      null,
      0,
      60,
    );
    expect(container.textContent).toContain("1 item");
  });

  it("opens photo viewer on photo click, opens video directly, and downloads selected items", async () => {
    const items = [makeMediaItem(1, "photo"), makeMediaItem(2, "video")];
    mockPhoneListMedia.mockResolvedValue({ items, total: 2 });

    await act(async () => {
      root.render(<GalleryView serial="serial-1" />);
    });

    // Video opens directly on PC
    const videoBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Video VID_2.mp4"]',
    );
    await act(async () => {
      videoBtn?.click();
    });
    expect(mockPhoneOpenMedia).toHaveBeenCalledWith("serial-1", "m-2");

    // Photo opens in-app viewer
    const photoBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Photo IMG_1.jpg"]',
    );
    await act(async () => {
      photoBtn?.click();
    });
    expect(container.querySelector('[role="dialog"]')).not.toBeNull();
    expect(container.textContent).toContain("Open on PC");

    // Multi-select Download N
    const closeBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Close"),
    );
    await act(async () => {
      closeBtn?.click();
    });

    const selectPhoto = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Select IMG_1.jpg"]',
    );
    await act(async () => {
      selectPhoto?.click();
    });

    const downloadBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Download 1"),
    );
    await act(async () => {
      downloadBtn?.click();
    });

    expect(mockOpenDialog).toHaveBeenCalled();
    expect(mockPullDeviceFile).toHaveBeenCalledWith(
      "serial-1",
      "/storage/emulated/0/DCIM/Camera/IMG_1.jpg",
      "C:\\Downloads",
    );
    expect(usePhoneHubStore.getState().lastGalleryDownloadDir).toBe("C:\\Downloads");
  });

  it("filters AppsView by search and scope and launches selected app", async () => {
    await act(async () => {
      root.render(<AppsView serial="serial-1" />);
    });

    expect(container.textContent).toContain("Chat");
    expect(container.textContent).toContain("Notes");
    expect(container.textContent).not.toContain("Settings");

    const search = container.querySelector<HTMLInputElement>("#apps-search");
    await act(async () => {
      if (search) {
        const setter = Object.getOwnPropertyDescriptor(
          HTMLInputElement.prototype,
          "value",
        )?.set;
        setter?.call(search, "notes");
        search.dispatchEvent(new Event("input", { bubbles: true }));
      }
    });

    expect(container.textContent).toContain("Notes");
    expect(container.textContent).not.toContain("Chat");

    const openBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Open Notes"]',
    );
    await act(async () => {
      openBtn?.click();
    });

    expect(mockPhoneOpenApp).toHaveBeenCalledWith("serial-1", "app.example.notes");
  });

  describe("pinned apps", () => {
    const pinnedSection = () =>
      container.querySelector<HTMLElement>('section[aria-label="Pinned apps"]');

    async function renderApps() {
      await act(async () => {
        root.render(<AppsView serial="serial-1" />);
      });
    }

    async function click(label: string) {
      const btn = container.querySelector<HTMLButtonElement>(
        `button[aria-label="${label}"]`,
      );
      expect(btn).not.toBeNull();
      await act(async () => {
        btn?.click();
      });
    }

    beforeEach(() => {
      pinnedState = [];
      mockPhoneSetAppPinned.mockClear();
      mockPhoneGetPinnedApps.mockClear();
    });

    it("hides the pinned section when nothing is pinned", async () => {
      await renderApps();
      expect(pinnedSection()).toBeNull();
      expect(container.querySelector('button[aria-label="Pin Chat"]')).not.toBeNull();
    });

    it("shows pinned apps sorted by label above the regular list", async () => {
      pinnedState = ["app.example.notes", "app.example.chat"];
      await renderApps();
      const section = pinnedSection();
      expect(section).not.toBeNull();
      const names = Array.from(section!.querySelectorAll("li")).map((li) => li.textContent);
      expect(names).toHaveLength(2);
      expect(names[0]).toContain("Chat");
      expect(names[1]).toContain("Notes");
      expect(section!.querySelector('button[aria-label="Unpin Chat"]')).not.toBeNull();
      const heading = Array.from(container.querySelectorAll("h2")).find(
        (h) => h.textContent === "User apps",
      );
      expect(heading).toBeDefined();
      expect(
        section!.compareDocumentPosition(heading!) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    });

    it("filters pinned apps by search and hides section when none match", async () => {
      pinnedState = ["app.example.notes", "app.example.chat"];
      await renderApps();
      const search = container.querySelector<HTMLInputElement>("#apps-search");
      const type = async (v: string) =>
        act(async () => {
          const setter = Object.getOwnPropertyDescriptor(
            HTMLInputElement.prototype,
            "value",
          )?.set;
          setter?.call(search, v);
          search?.dispatchEvent(new Event("input", { bubbles: true }));
        });
      await type("notes");
      expect(pinnedSection()?.querySelectorAll("li")).toHaveLength(1);
      expect(pinnedSection()?.textContent).toContain("Notes");
      expect(pinnedSection()?.textContent).not.toContain("Chat");
      await type("zzz");
      expect(pinnedSection()).toBeNull();
    });

    it("does not render pins for packages that are not installed", async () => {
      pinnedState = ["app.example.gone"];
      await renderApps();
      expect(pinnedSection()).toBeNull();
    });

    it("toggles pin optimistically and persists via the service", async () => {
      await renderApps();
      await click("Pin Notes");
      expect(mockPhoneSetAppPinned).toHaveBeenCalledWith("serial-1", "app.example.notes", true);
      expect(pinnedSection()?.textContent).toContain("Notes");
      await click("Unpin Notes");
      expect(mockPhoneSetAppPinned).toHaveBeenLastCalledWith(
        "serial-1",
        "app.example.notes",
        false,
      );
      expect(pinnedSection()).toBeNull();
    });

    it("reverts the optimistic pin when persisting fails", async () => {
      mockPhoneSetAppPinned.mockImplementationOnce(() => Promise.reject(new Error("boom")));
      await renderApps();
      await click("Pin Chat");
      expect(pinnedSection()).toBeNull();
      expect(container.textContent).toContain("boom");
    });
  });

  it("caps thumbnail requests at 4 concurrent, uses async/lazy sized imgs, and never uses original device path", async () => {
    vi.useFakeTimers();
    try {
      const items = Array.from({ length: 12 }, (_, i) => makeMediaItem(i, "photo"));
      mockPhoneListMedia.mockResolvedValue({ items, total: 12 });

      let inFlight = 0;
      let peakInFlight = 0;
      const resolvers: Array<() => void> = [];
      mockPhoneMediaThumbnail.mockImplementation((_serial: unknown, id: unknown) => {
        inFlight += 1;
        peakInFlight = Math.max(peakInFlight, inFlight);
        return new Promise<string>((resolve) => {
          resolvers.push(() => {
            inFlight -= 1;
            resolve(`C:\\cache\\phone_hub_thumbs\\abc\\${String(id)}-1.jpg`);
          });
        });
      });

      await act(async () => {
        root.render(<GalleryView serial="serial-1" />);
      });

      // Only 4 in flight initially even though 12 tiles are visible.
      expect(peakInFlight).toBe(4);
      expect(mockPhoneMediaThumbnail).toHaveBeenCalledTimes(4);

      // Drain all queued batches.
      while (resolvers.length > 0) {
        const batch = resolvers.splice(0, resolvers.length);
        await act(async () => {
          for (const done of batch) done();
          await Promise.resolve();
        });
      }

      await act(async () => {
        vi.advanceTimersByTime(100);
      });

      expect(peakInFlight).toBe(4);
      expect(mockPhoneMediaThumbnail).toHaveBeenCalledTimes(12);

      const imgs = Array.from(container.querySelectorAll<HTMLImageElement>("img.photo-tile-img"));
      expect(imgs.length).toBe(12);
      for (const img of imgs) {
        expect(img.getAttribute("decoding")).toBe("async");
        expect(img.getAttribute("loading")).toBe("lazy");
        expect(Number(img.getAttribute("width"))).toBeGreaterThan(0);
        expect(Number(img.getAttribute("height"))).toBeGreaterThan(0);
        expect(img.getAttribute("src")).toContain("phone_hub_thumbs");
        expect(img.getAttribute("src")).not.toContain("/storage/emulated/0/");
      }
    } finally {
      vi.useRealTimers();
    }
  });

  it("toggles Blur thumbnails on photo/video tiles, leaves viewer unblurred, and closes viewer on Escape", async () => {
    const items = [makeMediaItem(1, "photo"), makeMediaItem(2, "video")];
    mockPhoneListMedia.mockResolvedValue({ items, total: 2 });
    mockPhoneMediaThumbnail.mockResolvedValue("C:\\cache\\phone_hub_thumbs\\abc\\m-1-1.jpg");

    await act(async () => {
      root.render(<GalleryView serial="serial-1" />);
    });

    const toggleBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Blur thumbnails"),
    );
    expect(toggleBtn).toBeDefined();
    expect(toggleBtn?.getAttribute("aria-pressed")).toBe("false");
    expect(container.querySelectorAll(".gallery-tile-blur").length).toBe(0);

    // Enable blur toggle -> persists to store and adds gallery-tile-blur to both photo & video tiles
    await act(async () => {
      toggleBtn?.click();
    });
    expect(toggleBtn?.getAttribute("aria-pressed")).toBe("true");
    expect(usePhoneHubStore.getState().blurThumbnails).toBe(true);
    expect(container.querySelectorAll(".gallery-tile-blur").length).toBe(2);

    // Open photo viewer -> viewer modal is NOT inside .gallery-tile-blur
    const photoBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label^="Photo IMG_1.jpg"]',
    );
    await act(async () => {
      photoBtn?.click();
    });
    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(dialog?.closest(".gallery-tile-blur")).toBeNull();
    expect(dialog?.querySelector(".gallery-tile-blur")).toBeNull();

    // Pressing Escape closes the photo viewer
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(container.querySelector('[role="dialog"]')).toBeNull();

    // Disable blur toggle -> removes gallery-tile-blur
    await act(async () => {
      toggleBtn?.click();
    });
    expect(toggleBtn?.getAttribute("aria-pressed")).toBe("false");
    expect(usePhoneHubStore.getState().blurThumbnails).toBe(false);
    expect(container.querySelectorAll(".gallery-tile-blur").length).toBe(0);
  });
});
