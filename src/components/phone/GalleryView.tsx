import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { downloadDir } from "@tauri-apps/api/path";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Check,
  Download,
  Eye,
  EyeOff,
  Film,
  Image as ImageIcon,
  Images,
  Loader2,
  Play,
  RefreshCw,
  X,
} from "lucide-react";
import { pullDeviceFile } from "@/lib/tauri";
import { reportDeviceError } from "@/lib/deviceHealth";
import { selectClass, optionClass } from "@/lib/styles";
import {
  phoneListAlbums,
  phoneListMedia,
  phoneMediaThumbnail,
  phoneOpenMedia,
} from "@/services/phoneHubService";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import { logActivity } from "@/stores/activityStore";
import type { Album, MediaItem, MediaKind } from "@/types/phone";
import { ThumbnailQueue, type ThumbnailQueueEntry } from "./thumbnailQueue";

export const GALLERY_PAGE_SIZE = 60;
export const MIN_TILE_SIZE = 128;
export const GALLERY_TILE_GAP = 8;
const GALLERY_PADDING_X = 32;
const OVERSCAN_ROWS = 3;

export interface GalleryGridMetrics {
  cols: number;
  tileSize: number;
  rowStride: number;
}

/**
 * Compute responsive auto-fill column count, square tile size, and row stride
 * matching CSS `repeat(auto-fill, minmax(minTileSize, 1fr))` with `gap`.
 */
export function computeGalleryGridMetrics(
  containerWidth: number,
  minTileSize = MIN_TILE_SIZE,
  gap = GALLERY_TILE_GAP,
  paddingX = GALLERY_PADDING_X,
): GalleryGridMetrics {
  const availableWidth = Math.max(minTileSize, containerWidth - paddingX);
  const cols = Math.max(1, Math.floor((availableWidth + gap) / (minTileSize + gap)));
  const tileSize = (availableWidth - (cols - 1) * gap) / cols;
  return {
    cols,
    tileSize,
    rowStride: tileSize + gap,
  };
}

interface GalleryViewProps {
  serial: string;
  onAlbumChange?: (albumLabel: string) => void;
}

function formatDuration(ms: number | null): string {
  if (ms == null || ms < 0) return "0:00";
  const totalSecs = Math.floor(ms / 1000);
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `${mins}:${String(secs).padStart(2, "0")}`;
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const mb = bytes / (1024 * 1024);
  if (mb >= 1) return `${mb.toFixed(1)} MB`;
  const kb = bytes / 1024;
  return `${Math.round(kb)} KB`;
}

function formatDate(ms: number): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

interface GalleryTileProps {
  item: MediaItem;
  thumbSrc: string | undefined;
  isSelected: boolean;
  blurThumbnails: boolean;
  tileSizePx: number;
  onTileClick: (item: MediaItem) => void;
  onToggleSelect: (id: string) => void;
}

const GalleryTile = memo(function GalleryTile({
  item,
  thumbSrc,
  isSelected,
  blurThumbnails,
  tileSizePx,
  onTileClick,
  onToggleSelect,
}: GalleryTileProps) {
  return (
    <div
      className={`group relative aspect-square w-full overflow-hidden rounded-lg border bg-ink-900 ${
        blurThumbnails ? "gallery-tile-blur " : ""
      }${isSelected ? "border-gold-400" : "border-ink-700 hover:border-gold-400"}`}
    >
      <button
        type="button"
        onClick={() => onTileClick(item)}
        aria-label={
          item.kind === "video"
            ? `Video ${item.name}, ${formatDuration(item.durationMs)}`
            : `Photo ${item.name}`
        }
        className="btn-tactile gallery-tile-btn relative block h-full w-full cursor-pointer"
      >
        <span className="gallery-tile-content block h-full w-full">
          {item.kind === "photo" && thumbSrc ? (
            <img
              src={thumbSrc}
              alt={item.name}
              width={tileSizePx}
              height={tileSizePx}
              decoding="async"
              loading="lazy"
              className="photo-tile-img h-full w-full object-cover"
            />
          ) : (
            <span className="flex h-full w-full items-center justify-center text-text-muted">
              {item.kind === "video" ? (
                <Film className="h-5 w-5" />
              ) : (
                <ImageIcon className="h-5 w-5" />
              )}
            </span>
          )}

          {item.kind === "video" && (
            <>
              <span className="absolute left-1.5 top-1.5 flex h-[22px] w-[22px] items-center justify-center rounded-full bg-ink-950 text-text-strong">
                <Play className="h-3 w-3" />
              </span>
              <span className="absolute bottom-1.5 right-1.5 rounded bg-ink-950 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-text-strong group-hover:opacity-0">
                {formatDuration(item.durationMs)}
              </span>
            </>
          )}
        </span>

        <span className="hover-reveal absolute inset-x-0 bottom-0 truncate bg-ink-950 px-1.5 py-1 font-mono text-[10px] text-text-strong">
          {item.name}
        </span>
      </button>

      {/* Selection toggle */}
      <button
        type="button"
        aria-label={isSelected ? `Deselect ${item.name}` : `Select ${item.name}`}
        aria-pressed={isSelected}
        onClick={(e) => {
          e.stopPropagation();
          onToggleSelect(item.id);
        }}
        className={`absolute right-1.5 top-1.5 flex h-5 w-5 items-center justify-center rounded border text-[10px] cursor-pointer ${
          isSelected
            ? "border-gold-400 bg-gold-400 text-ink-950 opacity-100"
            : "hover-reveal border-ink-700 bg-ink-950/90 text-text-strong"
        }`}
      >
        {isSelected && <Check className="h-3 w-3" />}
      </button>
    </div>
  );
});

export function GalleryView({ serial, onAlbumChange }: GalleryViewProps) {
  const lastDownloadDir = usePhoneHubStore((s) => s.lastGalleryDownloadDir);
  const setLastDownloadDir = usePhoneHubStore((s) => s.setLastGalleryDownloadDir);
  const blurThumbnails = usePhoneHubStore((s) => s.blurThumbnails);
  const setBlurThumbnails = usePhoneHubStore((s) => s.setBlurThumbnails);

  const [kind, setKind] = useState<MediaKind>("all");
  const [album, setAlbum] = useState<string>("__all");
  const [albums, setAlbums] = useState<Album[]>([]);
  const [items, setItems] = useState<MediaItem[]>([]);
  const [total, setTotal] = useState<number>(0);
  const [initialLoading, setInitialLoading] = useState<boolean>(true);
  const [loadingMore, setLoadingMore] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const thumbQueueRef = useRef<ThumbnailQueue | null>(null);
  const scrollRafRef = useRef<number | null>(null);

  const [viewerItem, setViewerItem] = useState<MediaItem | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [downloadStatus, setDownloadStatus] = useState<{
    state: "idle" | "downloading" | "done" | "failed";
    message: string;
  }>({ state: "idle", message: "" });
  const [statusToast, setStatusToast] = useState<string | null>(null);

  // Virtualization / windowing state
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportSize, setViewportSize] = useState({ width: 800, height: 600 });

  const queryGenerationRef = useRef(0);

  // Notify parent header subtitle when album changes
  useEffect(() => {
    onAlbumChange?.(album === "__all" ? "All albums" : album);
  }, [album, onAlbumChange]);

  // Load albums and reset thumbnail queue on serial change
  useEffect(() => {
    let cancelled = false;
    setAlbums([]);
    setAlbum("__all");
    setKind("all");
    setThumbs({});

    const queue = new ThumbnailQueue({
      maxConcurrent: 4,
      batchIntervalMs: 80,
      onBatch: (updates) => {
        if (!cancelled) {
          setThumbs((prev) => ({ ...prev, ...updates }));
        }
      },
    });
    thumbQueueRef.current = queue;

    phoneListAlbums(serial)
      .then((list) => {
        if (!cancelled) setAlbums(list);
      })
      .catch(() => {
        if (!cancelled) setAlbums([]);
      });

    return () => {
      cancelled = true;
      queue.destroy();
      if (thumbQueueRef.current === queue) {
        thumbQueueRef.current = null;
      }
    };
  }, [serial]);

  useEffect(() => {
    return () => {
      if (scrollRafRef.current !== null) {
        cancelAnimationFrame(scrollRafRef.current);
        scrollRafRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    if (!viewerItem) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        setViewerItem(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [viewerItem]);

  const fetchPage = useCallback(
    async (offset: number, reset: boolean) => {
      const gen = reset ? ++queryGenerationRef.current : queryGenerationRef.current;
      if (reset) {
        setInitialLoading(true);
        setLoadingMore(false);
        setItems([]);
        setTotal(0);
        setSelectedIds(new Set());
        setScrollTop(0);
        if (scrollContainerRef.current) {
          scrollContainerRef.current.scrollTop = 0;
        }
      } else {
        setLoadingMore(true);
      }
      setError(null);

      try {
        const query = () =>
          phoneListMedia(
            serial,
            kind,
            album === "__all" ? null : album,
            offset,
            GALLERY_PAGE_SIZE,
          );
        let res;
        try {
          res = await query();
        } catch (firstErr) {
          // Dropped wireless transport: reconnect via mDNS, then retry once.
          if (!(await reportDeviceError(firstErr, serial))) throw firstErr;
          res = await query();
        }
        if (gen !== queryGenerationRef.current) return;

        setTotal(res.total);
        setItems((prev) => {
          if (reset) return res.items;
          const seen = new Set(prev.map((x) => x.id));
          const fresh = res.items.filter((x) => !seen.has(x.id));
          return [...prev, ...fresh];
        });
      } catch (e) {
        if (gen !== queryGenerationRef.current) return;
        setError(String(e));
      } finally {
        if (gen === queryGenerationRef.current) {
          setInitialLoading(false);
          setLoadingMore(false);
        }
      }
    },
    [serial, kind, album],
  );

  // Reset & load first page whenever serial, kind, or album changes
  useEffect(() => {
    void fetchPage(0, true);
  }, [fetchPage]);

  // Measure scroll container for bounded DOM windowing
  useEffect(() => {
    const el = scrollContainerRef.current;
    if (!el) return;

    const updateSize = () => {
      if (el.clientWidth > 0 && el.clientHeight > 0) {
        setViewportSize({ width: el.clientWidth, height: el.clientHeight });
        setScrollTop(el.scrollTop);
      }
    };
    updateSize();

    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(updateSize);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const hasMore = items.length < total;

  // IntersectionObserver sentinel for infinite scroll
  useEffect(() => {
    const sentinel = sentinelRef.current;
    const root = scrollContainerRef.current;
    if (!sentinel || !hasMore || loadingMore || initialLoading) return;
    if (typeof IntersectionObserver === "undefined") return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          void fetchPage(items.length, false);
        }
      },
      { root, rootMargin: "0px 0px 80px 0px" },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [hasMore, loadingMore, initialLoading, items.length, fetchPage]);

  // Window calculation
  const { cols, tileSize, rowStride } = computeGalleryGridMetrics(viewportSize.width);
  const tileSizePx = Math.max(1, Math.round(tileSize));
  const gridStyle = {
    gridTemplateColumns: `repeat(auto-fill, minmax(${MIN_TILE_SIZE}px, 1fr))`,
  };
  const totalRows = Math.ceil(items.length / cols);
  const coreStartRow = Math.max(0, Math.floor(scrollTop / rowStride));
  const coreEndRow = Math.min(
    totalRows,
    Math.ceil((scrollTop + viewportSize.height) / rowStride),
  );
  const startRow = Math.max(0, coreStartRow - OVERSCAN_ROWS);
  const endRow = Math.min(totalRows, coreEndRow + OVERSCAN_ROWS);
  const visibleItems = useMemo(
    () => items.slice(startRow * cols, endRow * cols),
    [items, startRow, endRow, cols],
  );

  // Sync desired thumbnails into the bounded queue (visible rows first, then overscan).
  // Intentionally does NOT depend on `thumbs` so completing one tile never re-triggers the effect.
  const fetchThumbnail = useCallback(
    (id: string) =>
      phoneMediaThumbnail(serial, id).then((localPath) =>
        localPath ? convertFileSrc(localPath) : null,
      ),
    [serial],
  );

  useEffect(() => {
    const queue = thumbQueueRef.current;
    if (!queue) return;

    const entries: ThumbnailQueueEntry[] = [];
    if (viewerItem && viewerItem.kind === "photo") {
      entries.push({ id: viewerItem.id, priority: -1 });
    }

    const baseIndex = startRow * cols;
    for (let idx = 0; idx < visibleItems.length; idx += 1) {
      const item = visibleItems[idx];
      if (item.kind !== "photo") continue;
      const globalIdx = baseIndex + idx;
      const row = Math.floor(globalIdx / cols);
      let priority: number;
      if (row >= coreStartRow && row < coreEndRow) {
        priority = idx;
      } else if (row >= coreEndRow) {
        priority = 10_000 + idx;
      } else {
        priority = 20_000 + (coreStartRow * cols - globalIdx);
      }
      entries.push({ id: item.id, priority });
    }

    queue.syncDesired(entries, fetchThumbnail);
  }, [
    visibleItems,
    viewerItem,
    startRow,
    coreStartRow,
    coreEndRow,
    cols,
    fetchThumbnail,
  ]);

  const totalAllAlbums = useMemo(
    () => albums.reduce((sum, a) => sum + a.photoCount + a.videoCount, 0),
    [albums],
  );

  const handleOpenOnPc = useCallback(
    async (item: MediaItem) => {
      setStatusToast(`Opening ${item.name} on PC…`);
      try {
        await phoneOpenMedia(serial, item.id);
      } catch (e) {
        setError(String(e));
      }
    },
    [serial],
  );

  const handleTileClick = useCallback(
    (item: MediaItem) => {
      if (item.kind === "video") {
        // §12: videos open directly in the PC's default player
        void handleOpenOnPc(item);
        return;
      }
      setViewerItem(item);
    },
    [handleOpenOnPc],
  );

  const toggleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const handleScroll = useCallback(() => {
    if (scrollRafRef.current !== null) return;
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = null;
      const el = scrollContainerRef.current;
      if (el) {
        setScrollTop(el.scrollTop);
      }
    });
  }, []);

  async function handleDownloadItems(targets: MediaItem[]) {
    if (targets.length === 0) return;
    let defaultPath = lastDownloadDir ?? undefined;
    if (!defaultPath) {
      defaultPath = await downloadDir().catch(() => undefined);
    }
    const dest = await openDialog({
      directory: true,
      multiple: false,
      defaultPath,
    });
    if (!dest || typeof dest !== "string") return;

    setLastDownloadDir(dest);
    setDownloadStatus({
      state: "downloading",
      message: `Downloading 0/${targets.length}…`,
    });

    let completed = 0;
    try {
      for (const item of targets) {
        await pullDeviceFile(serial, item.path, dest);
        completed += 1;
        setDownloadStatus({
          state: "downloading",
          message: `Downloading ${completed}/${targets.length}…`,
        });
        logActivity({
          kind: "files",
          title: `Downloaded ${item.name}`,
          detail: dest,
        });
      }
      setDownloadStatus({
        state: "done",
        message: `Downloaded ${completed} item${completed > 1 ? "s" : ""} to ${dest}`,
      });
      setSelectedIds(new Set());
    } catch (e) {
      setDownloadStatus({
        state: "failed",
        message: `Download failed (${completed}/${targets.length}): ${String(e)}`,
      });
    }
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-3 border-b border-ink-700 bg-ink-900 px-4 py-2.5">
        <div
          role="group"
          aria-label="Media type"
          className="inline-flex items-center gap-0.5 rounded-lg border border-ink-700 bg-ink-950 p-0.5"
        >
          {(["all", "photo", "video"] as MediaKind[]).map((k) => {
            const label = k === "all" ? "All" : k === "photo" ? "Photos" : "Videos";
            const active = kind === k;
            return (
              <button
                key={k}
                type="button"
                aria-pressed={active}
                onClick={() => setKind(k)}
                className={`btn-tactile rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
                  active
                    ? "bg-ink-800 text-gold-400"
                    : "text-text-muted hover:text-text-strong"
                }`}
              >
                {label}
              </button>
            );
          })}
        </div>

        <div className="flex items-center gap-2">
          <label htmlFor="gallery-album" className="text-xs text-text-muted">
            Album
          </label>
          <select
            id="gallery-album"
            value={album}
            onChange={(e) => setAlbum(e.target.value)}
            className={`${selectClass} !w-auto min-w-[200px] !py-1`}
          >
            <option value="__all" className={optionClass}>
              All albums{totalAllAlbums > 0 ? ` (${totalAllAlbums})` : ""}
            </option>
            {albums.map((a) => (
              <option key={a.name} value={a.name} className={optionClass}>
                {a.name} ({a.photoCount + a.videoCount})
              </option>
            ))}
          </select>
        </div>

        <button
          type="button"
          aria-pressed={blurThumbnails}
          onClick={() => setBlurThumbnails(!blurThumbnails)}
          className={`btn-tactile inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-xs font-medium cursor-pointer ${
            blurThumbnails
              ? "border-gold-400 bg-ink-800 text-gold-400"
              : "border-ink-700 bg-ink-950 text-text-muted hover:border-text-muted hover:text-text-strong"
          }`}
        >
          {blurThumbnails ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
          Blur thumbnails
        </button>

        <div className="flex-1" />

        {selectedIds.size > 0 && (
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={downloadStatus.state === "downloading"}
              onClick={() =>
                void handleDownloadItems(items.filter((i) => selectedIds.has(i.id)))
              }
              className="btn-tactile inline-flex items-center gap-1.5 rounded-lg border border-gold-400 bg-gold-400 px-2.5 py-1 text-xs font-semibold text-ink-950 hover:border-gold-600 hover:bg-gold-600 disabled:opacity-50 cursor-pointer"
            >
              <Download className="h-3.5 w-3.5" />
              Download {selectedIds.size}
            </button>
            <button
              type="button"
              onClick={() => setSelectedIds(new Set())}
              className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-xs text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
            >
              Clear
            </button>
          </div>
        )}

        <span className="font-mono text-xs text-text-muted" role="status">
          {initialLoading ? "Loading…" : `${total} item${total === 1 ? "" : "s"}`}
        </span>

        <button
          type="button"
          disabled={initialLoading}
          onClick={() => {
            setThumbs({});
            thumbQueueRef.current?.reset();
            void fetchPage(0, true);
          }}
          aria-label="Refresh gallery"
          className="btn-tactile inline-flex items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-xs text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${initialLoading ? "animate-spin" : ""}`} />
        </button>
      </div>

      {/* Download / error feedback */}
      {downloadStatus.state !== "idle" && (
        <div
          role="status"
          className={`flex items-center justify-between border-b px-4 py-1.5 text-xs ${
            downloadStatus.state === "failed"
              ? "border-status-danger/40 bg-status-danger-bg text-status-danger"
              : downloadStatus.state === "done"
                ? "border-status-success/40 bg-status-success-bg text-status-success"
                : "border-ink-700 bg-ink-900 text-text-strong"
          }`}
        >
          <span className="truncate">{downloadStatus.message}</span>
          {downloadStatus.state !== "downloading" && (
            <button
              type="button"
              onClick={() => setDownloadStatus({ state: "idle", message: "" })}
              className="ml-2 text-text hover:text-text-strong cursor-pointer"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      )}

      {error && (
        <div
          role="alert"
          className="border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger"
        >
          {error}
        </div>
      )}

      {/* Scrollable grid */}
      <div
        ref={scrollContainerRef}
        onScroll={handleScroll}
        className="relative flex-1 overflow-y-auto p-4"
      >
        {initialLoading ? (
          <div
            className="grid gap-2"
            style={gridStyle}
          >
            {Array.from({ length: 24 }, (_, i) => (
              <div
                key={i}
                aria-hidden="true"
                className={`aspect-square w-full rounded-lg border border-ink-700 ${
                  i % 2 === 0 ? "bg-ink-900" : "bg-ink-800/60"
                }`}
              />
            ))}
            <span className="sr-only" role="status">
              Loading gallery
            </span>
          </div>
        ) : items.length === 0 ? (
          <div className="flex min-h-[320px] flex-col items-center justify-center rounded-xl border border-dashed border-ink-700 p-8 text-center">
            <Images className="mb-2 h-10 w-10 text-ink-700" />
            <h2 className="text-sm font-semibold text-text-strong">
              {album !== "__all" ? "This album is empty" : "Nothing here"}
            </h2>
            <p className="mt-1 max-w-md text-xs text-text">
              {album !== "__all"
                ? "No photos or videos match this filter in the selected album."
                : "No photos or videos found on this device."}
            </p>
            {(album !== "__all" || kind !== "all") && (
              <button
                type="button"
                onClick={() => {
                  setAlbum("__all");
                  setKind("all");
                }}
                className="btn-tactile mt-3 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text-strong hover:border-text-muted cursor-pointer"
              >
                Show all albums
              </button>
            )}
          </div>
        ) : (
          <>
            {/* Top spacer for virtualized rows */}
            {startRow > 0 && <div style={{ height: startRow * rowStride }} aria-hidden="true" />}

            <div
              aria-live="polite"
              className="grid gap-2"
              style={gridStyle}
            >
              {visibleItems.map((item) => (
                <GalleryTile
                  key={item.id}
                  item={item}
                  thumbSrc={thumbs[item.id]}
                  isSelected={selectedIds.has(item.id)}
                  blurThumbnails={blurThumbnails}
                  tileSizePx={tileSizePx}
                  onTileClick={handleTileClick}
                  onToggleSelect={toggleSelect}
                />
              ))}
            </div>

            {/* Bottom spacer for virtualized rows */}
            {endRow < totalRows && (
              <div
                style={{ height: (totalRows - endRow) * rowStride }}
                aria-hidden="true"
              />
            )}

            {/* Loading more skeletons / sentinel / end-of-list */}
            {loadingMore && (
              <div className="mt-2 space-y-2">
                <div
                  className="grid gap-2"
                  style={gridStyle}
                >
                  {Array.from({ length: 12 }, (_, i) => (
                    <div
                      key={i}
                      aria-hidden="true"
                      className="aspect-square w-full rounded-lg border border-ink-700 bg-ink-800/60"
                    />
                  ))}
                </div>
                <div
                  role="status"
                  className="flex items-center gap-2 text-xs text-text"
                >
                  <Loader2 className="h-3.5 w-3.5 animate-spin text-gold-400" />
                  <span>
                    Loading {Math.min(GALLERY_PAGE_SIZE, total - items.length)} more…
                  </span>
                </div>
              </div>
            )}

            {!loadingMore && hasMore && (
              <div ref={sentinelRef} className="mt-3 flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => void fetchPage(items.length, false)}
                  className="btn-tactile rounded-md border border-ink-700 bg-ink-800 px-3 py-1.5 text-[11px] font-medium text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
                >
                  Load more ({total - items.length} left)
                </button>
              </div>
            )}

            {!loadingMore && !hasMore && (
              <div className="mt-3 text-xs text-text-muted">
                End of album · {total} item{total === 1 ? "" : "s"}
              </div>
            )}
          </>
        )}

        {statusToast && (
          <div
            role="status"
            className="popover-panel pointer-events-none fixed bottom-6 left-1/2 -translate-x-1/2 z-30 rounded-lg border border-ink-700 bg-ink-900 px-3.5 py-2 text-xs text-text-strong shadow-elevated"
          >
            {statusToast}
          </div>
        )}
      </div>

      {/* Photo viewer panel */}
      {viewerItem && (
        <div
          className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-ink-950/90 p-6"
          onClick={() => setViewerItem(null)}
        >
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="gallery-viewer-title"
            className="modal-panel w-full max-w-2xl overflow-hidden rounded-xl border border-ink-700 bg-ink-900 shadow-elevated"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between gap-2 border-b border-ink-700 px-4 py-2.5">
              <h2
                id="gallery-viewer-title"
                className="truncate font-mono text-xs font-semibold text-text-strong"
              >
                {viewerItem.name}
              </h2>
              <button
                type="button"
                onClick={() => setViewerItem(null)}
                className="btn-tactile inline-flex items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-[11px] text-text hover:border-text-muted hover:text-text-strong cursor-pointer"
              >
                <X className="h-3.5 w-3.5" />
                Close
              </button>
            </div>

            <div className="p-4">
              <div className="mb-3.5 flex h-[300px] items-center justify-center overflow-hidden rounded-lg border border-ink-700 bg-ink-950">
                {thumbs[viewerItem.id] ? (
                  <img
                    src={thumbs[viewerItem.id]}
                    alt={viewerItem.name}
                    className="max-h-full max-w-full object-contain"
                  />
                ) : (
                  <ImageIcon className="h-10 w-10 text-ink-700" />
                )}
              </div>

              <dl className="grid grid-cols-[110px_1fr] gap-x-3 gap-y-1.5 text-xs">
                <dt className="text-text-muted">Album</dt>
                <dd className="text-text-strong">{viewerItem.album || "—"}</dd>
                <dt className="text-text-muted">Dimensions</dt>
                <dd className="text-text-strong tabular-nums">
                  {viewerItem.width && viewerItem.height
                    ? `${viewerItem.width} × ${viewerItem.height}`
                    : "—"}
                </dd>
                <dt className="text-text-muted">Size</dt>
                <dd className="text-text-strong tabular-nums">
                  {formatBytes(viewerItem.sizeBytes)}
                </dd>
                <dt className="text-text-muted">Taken</dt>
                <dd className="text-text-strong">
                  {formatDate(viewerItem.dateTakenMs)}
                </dd>
              </dl>

              <div className="mt-4 flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  onClick={() => void handleOpenOnPc(viewerItem)}
                  className="btn-tactile rounded-lg border border-gold-400 bg-gold-400 px-3 py-1.5 text-xs font-semibold text-ink-950 hover:border-gold-600 hover:bg-gold-600 cursor-pointer"
                >
                  Open on PC
                </button>
                <button
                  type="button"
                  disabled={downloadStatus.state === "downloading"}
                  onClick={() => void handleDownloadItems([viewerItem])}
                  className="btn-tactile inline-flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text-strong hover:border-text-muted disabled:opacity-50 cursor-pointer"
                >
                  <Download className="h-3.5 w-3.5" />
                  Download
                </button>
                <span className="text-[11px] text-text-muted">
                  Pulled to a temporary cache, then opened with your default PC app.
                </span>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
