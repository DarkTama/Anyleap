import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Download, FolderOpen, Image as ImageIcon, Loader2, RefreshCw, X } from "lucide-react";
import { cacheDevicePhoto, listDevicePhotos, pullDeviceFile } from "@/lib/tauri";
import { logActivity } from "@/stores/activityStore";
import type { DeviceEntry } from "@/lib/types";

const PAGE_SIZE = 60;

interface PhotosViewProps {
  serial: string;
}

export function PhotosView({ serial }: PhotosViewProps) {
  const [photos, setPhotos] = useState<DeviceEntry[]>([]);
  const [cachedSrcs, setCachedSrcs] = useState<Record<string, string>>({});
  const [cachedLocalPaths, setCachedLocalPaths] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [lightbox, setLightbox] = useState<DeviceEntry | null>(null);
  const [busyDownload, setBusyDownload] = useState(false);
  const [downloadedPath, setDownloadedPath] = useState<string | null>(null);

  const inFlightRef = useRef<Set<string>>(new Set());

  const loadPage = useCallback(
    async (offset: number, reset: boolean) => {
      setLoading(true);
      setError(null);
      try {
        const page = await listDevicePhotos(serial, offset, PAGE_SIZE);
        setPhotos((prev) => (reset ? page : [...prev, ...page]));
        setHasMore(page.length === PAGE_SIZE);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [serial],
  );

  useEffect(() => {
    setPhotos([]);
    setCachedSrcs({});
    setCachedLocalPaths({});
    setHasMore(true);
    inFlightRef.current.clear();
    void loadPage(0, true);
  }, [serial, loadPage]);

  const ensureCached = useCallback(
    async (entry: DeviceEntry) => {
      if (cachedSrcs[entry.path] || inFlightRef.current.has(entry.path)) return;
      inFlightRef.current.add(entry.path);
      try {
        const localPath = await cacheDevicePhoto(serial, entry.path, entry.modified_unix);
        setCachedLocalPaths((prev) => ({ ...prev, [entry.path]: localPath }));
        setCachedSrcs((prev) => ({ ...prev, [entry.path]: convertFileSrc(localPath) }));
      } catch {
        // Leave placeholder on individual pull failure
      } finally {
        inFlightRef.current.delete(entry.path);
      }
    },
    [serial, cachedSrcs],
  );

  async function handleDownloadLightbox() {
    if (!lightbox) return;
    const dest = await openDialog({ directory: true, multiple: false });
    if (!dest || typeof dest !== "string") return;
    setBusyDownload(true);
    setError(null);
    try {
      const saved = await pullDeviceFile(serial, lightbox.path, dest);
      setDownloadedPath(saved);
      logActivity({
        kind: "files",
        title: `Downloaded ${lightbox.name}`,
        detail: serial,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyDownload(false);
    }
  }

  async function handleOpenFolder() {
    if (!lightbox) return;
    const target = downloadedPath ?? cachedLocalPaths[lightbox.path];
    if (target) {
      await revealItemInDir(target).catch(() => {});
    }
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-ink-700 bg-ink-900 px-4 py-2">
        <span className="text-xs text-text-muted">
          Camera & Screenshots ({photos.length} loaded)
        </span>
        <button
          type="button"
          disabled={loading}
          onClick={() => void loadPage(0, true)}
          className="btn-tactile flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          Refresh
        </button>
      </div>

      {error && (
        <div className="border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-4">
        {photos.length === 0 && !loading ? (
          <div className="flex h-56 flex-col items-center justify-center text-xs text-text-muted">
            <ImageIcon className="mb-2 h-8 w-8 text-ink-700" />
            <p>No photos found in DCIM/Camera or Pictures/Screenshots</p>
          </div>
        ) : (
          <>
            <div className="grid grid-cols-3 gap-2 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6">
              {photos.map((entry) => (
                <PhotoTile
                  key={entry.path}
                  entry={entry}
                  src={cachedSrcs[entry.path]}
                  onVisible={() => void ensureCached(entry)}
                  onClick={() => {
                    setDownloadedPath(null);
                    setLightbox(entry);
                    void ensureCached(entry);
                  }}
                />
              ))}
            </div>

            {hasMore && (
              <div className="mt-4 flex justify-center">
                <button
                  type="button"
                  disabled={loading}
                  onClick={() => void loadPage(photos.length, false)}
                  className="btn-tactile flex items-center gap-2 rounded-lg border border-ink-700 bg-ink-800 px-4 py-1.5 text-xs font-medium text-text-strong hover:border-text-muted disabled:opacity-50 cursor-pointer"
                >
                  {loading && <Loader2 className="h-3.5 w-3.5 animate-spin text-gold-400" />}
                  Load more
                </button>
              </div>
            )}
          </>
        )}
      </div>

      {lightbox && (
        <div
          className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-ink-950/90 p-6"
          onClick={() => setLightbox(null)}
        >
          <div
            className="modal-panel relative flex max-h-[90vh] max-w-4xl flex-col overflow-hidden rounded-xl border border-ink-700 bg-ink-900 shadow-elevated"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-ink-700 px-4 py-2.5">
              <span className="truncate font-mono text-xs text-text-strong">
                {lightbox.name}
              </span>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  disabled={busyDownload}
                  onClick={() => void handleDownloadLightbox()}
                  className="btn-tactile flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs font-medium text-text-strong hover:border-text-muted cursor-pointer"
                >
                  {busyDownload ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin text-gold-400" />
                  ) : (
                    <Download className="h-3.5 w-3.5" />
                  )}
                  Download
                </button>
                <button
                  type="button"
                  disabled={!downloadedPath && !cachedLocalPaths[lightbox.path]}
                  onClick={() => void handleOpenFolder()}
                  className="btn-tactile flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs font-medium text-text-strong hover:border-text-muted disabled:opacity-40 cursor-pointer"
                >
                  <FolderOpen className="h-3.5 w-3.5" />
                  Open folder
                </button>
                <button
                  type="button"
                  onClick={() => setLightbox(null)}
                  className="btn-tactile rounded-md p-1 text-text-muted hover:bg-ink-800 hover:text-text-strong cursor-pointer"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>
            </div>

            <div className="flex flex-1 items-center justify-center overflow-hidden bg-black p-4">
              {cachedSrcs[lightbox.path] ? (
                <img
                  src={cachedSrcs[lightbox.path]}
                  alt={lightbox.name}
                  className="max-h-[75vh] max-w-full object-contain"
                />
              ) : (
                <div className="flex h-64 w-64 items-center justify-center text-text-muted">
                  <Loader2 className="h-6 w-6 animate-spin text-gold-400" />
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function PhotoTile({
  entry,
  src,
  onVisible,
  onClick,
}: {
  entry: DeviceEntry;
  src?: string;
  onVisible: () => void;
  onClick: () => void;
}) {
  const ref = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el || src) return;
    if (typeof IntersectionObserver === "undefined") {
      onVisible();
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          onVisible();
          observer.disconnect();
        }
      },
      { rootMargin: "120px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [src, onVisible]);

  return (
    <button
      ref={ref}
      type="button"
      onClick={onClick}
      className="btn-tactile group relative aspect-square overflow-hidden rounded-lg border border-ink-700 bg-ink-900 hover:border-gold-400 cursor-pointer"
    >
      {src ? (
        <img
          src={src}
          alt={entry.name}
          className="photo-tile-img h-full w-full object-cover"
        />
      ) : (
        <div className="flex h-full w-full items-center justify-center text-text-muted">
          <ImageIcon className="h-5 w-5" />
        </div>
      )}
    </button>
  );
}
