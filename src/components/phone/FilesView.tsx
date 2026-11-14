import { useCallback, useEffect, useState } from "react";
import {
  ChevronRight,
  Download,
  File,
  Folder,
  Loader2,
  RefreshCw,
  Trash2,
  Upload,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  deleteDevicePath,
  listDeviceDir,
  pullDeviceFile,
  pushToDevice,
} from "@/lib/tauri";
import { logActivity } from "@/stores/activityStore";
import type { DeviceEntry } from "@/lib/types";

const DEVICE_ROOT = "/storage/emulated/0";

const QUICK_ROOTS = [
  { label: "Internal storage", path: DEVICE_ROOT },
  { label: "Download", path: `${DEVICE_ROOT}/Download` },
  { label: "DCIM", path: `${DEVICE_ROOT}/DCIM` },
  { label: "Documents", path: `${DEVICE_ROOT}/Documents` },
];

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let val = bytes;
  let idx = 0;
  while (val >= 1024 && idx < units.length - 1) {
    val /= 1024;
    idx++;
  }
  return `${val < 10 && idx > 0 ? val.toFixed(1) : Math.round(val)} ${units[idx]}`;
}

function formatDate(unix: number): string {
  if (!unix) return "—";
  return new Date(unix * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

interface FilesViewProps {
  serial: string;
}

export function FilesView({ serial }: FilesViewProps) {
  const [currentPath, setCurrentPath] = useState(DEVICE_ROOT);
  const [entries, setEntries] = useState<DeviceEntry[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [busyOp, setBusyOp] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [dragOver, setDragOver] = useState(false);

  const loadDir = useCallback(
    async (targetPath: string) => {
      setLoading(true);
      setError(null);
      setConfirmDelete(false);
      try {
        const list = await listDeviceDir(serial, targetPath);
        setEntries(list);
        setCurrentPath(targetPath);
        setSelected(new Set());
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [serial],
  );

  useEffect(() => {
    void loadDir(DEVICE_ROOT);
  }, [serial, loadDir]);

  const handleUploadPaths = useCallback(
    async (localPaths: string[]) => {
      if (!localPaths.length) return;
      setBusyOp(`Uploading ${localPaths.length} item(s)...`);
      setError(null);
      setStatusMsg(null);
      try {
        await pushToDevice(serial, localPaths, currentPath);
        logActivity({
          kind: "files",
          title: `Sent ${localPaths.length} file(s) to phone`,
          detail: serial,
        });
        setStatusMsg(`Uploaded ${localPaths.length} item(s)`);
        await loadDir(currentPath);
      } catch (e) {
        setError(String(e));
      } finally {
        setBusyOp(null);
      }
    },
    [serial, currentPath, loadDir],
  );

  // OS drag-and-drop onto the file list via Tauri webview event
  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = getCurrentWebview().onDragDropEvent((event) => {
      if (cancelled) return;
      if (event.payload.type === "over" || event.payload.type === "enter") {
        setDragOver(true);
      } else if (event.payload.type === "leave") {
        setDragOver(false);
      } else if (event.payload.type === "drop") {
        setDragOver(false);
        if (event.payload.paths.length > 0) {
          void handleUploadPaths(event.payload.paths);
        }
      }
    });
    return () => {
      cancelled = true;
      void unlistenPromise.then((un) => un());
    };
  }, [handleUploadPaths]);

  async function handleDownloadSelected() {
    const selectedEntries = entries.filter((e) => selected.has(e.path));
    if (!selectedEntries.length) return;
    const dest = await openDialog({ directory: true, multiple: false });
    if (!dest || typeof dest !== "string") return;

    setBusyOp(`Downloading ${selectedEntries.length} item(s)...`);
    setError(null);
    setStatusMsg(null);
    try {
      for (const entry of selectedEntries) {
        await pullDeviceFile(serial, entry.path, dest);
        logActivity({
          kind: "files",
          title: `Downloaded ${entry.name}`,
          detail: serial,
        });
      }
      setStatusMsg(`Downloaded ${selectedEntries.length} item(s) to ${dest}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyOp(null);
    }
  }

  async function handleUploadClick() {
    const picked = await openDialog({ multiple: true, directory: false });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    await handleUploadPaths(paths);
  }

  async function handleDeleteConfirmed() {
    const targets = Array.from(selected);
    if (!targets.length) return;
    setConfirmDelete(false);
    setBusyOp(`Deleting ${targets.length} item(s)...`);
    setError(null);
    setStatusMsg(null);
    try {
      for (const path of targets) {
        await deleteDevicePath(serial, path);
        const entry = entries.find((e) => e.path === path);
        logActivity({
          kind: "files",
          title: `Deleted ${entry?.name ?? path}`,
          detail: serial,
        });
      }
      setStatusMsg(`Deleted ${targets.length} item(s)`);
      await loadDir(currentPath);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyOp(null);
    }
  }

  function toggleSelect(path: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    setConfirmDelete(false);
  }

  function toggleAll() {
    if (selected.size === entries.length && entries.length > 0) {
      setSelected(new Set());
    } else {
      setSelected(new Set(entries.map((e) => e.path)));
    }
    setConfirmDelete(false);
  }

  const relativeSegments = currentPath
    .slice(DEVICE_ROOT.length)
    .split("/")
    .filter(Boolean);

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* Quick roots & actions bar */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ink-700 bg-ink-900 px-4 py-2.5">
        <div className="flex flex-wrap items-center gap-1.5">
          {QUICK_ROOTS.map((root) => (
            <button
              key={root.path}
              type="button"
              onClick={() => void loadDir(root.path)}
              className={`btn-tactile rounded-md px-2.5 py-1 text-xs font-medium cursor-pointer ${
                currentPath === root.path
                  ? "bg-ink-800 text-gold-400"
                  : "text-text-muted hover:bg-ink-800 hover:text-text-strong"
              }`}
            >
              {root.label}
            </button>
          ))}
        </div>

        <div className="flex items-center gap-2">
          {busyOp && (
            <span className="flex items-center gap-1.5 text-xs text-mist-300">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {busyOp}
            </span>
          )}
          <button
            type="button"
            disabled={!!busyOp}
            onClick={() => void handleUploadClick()}
            className="btn-tactile flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs font-medium text-text-strong hover:border-text-muted disabled:opacity-50 cursor-pointer"
          >
            <Upload className="h-3.5 w-3.5" />
            Upload
          </button>
          <button
            type="button"
            disabled={selected.size === 0 || !!busyOp}
            onClick={() => void handleDownloadSelected()}
            className="btn-tactile flex items-center gap-1.5 rounded-md border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs font-medium text-text-strong hover:border-text-muted disabled:opacity-40 cursor-pointer"
          >
            <Download className="h-3.5 w-3.5" />
            Download{selected.size > 0 ? ` (${selected.size})` : ""}
          </button>
          <button
            type="button"
            disabled={selected.size === 0 || !!busyOp}
            onClick={() => setConfirmDelete(true)}
            className="btn-tactile flex items-center gap-1.5 rounded-md border border-status-danger/40 bg-status-danger-bg px-2.5 py-1 text-xs font-medium text-status-danger hover:opacity-90 disabled:opacity-40 cursor-pointer"
          >
            <Trash2 className="h-3.5 w-3.5" />
            Delete
          </button>
          <button
            type="button"
            disabled={loading || !!busyOp}
            onClick={() => void loadDir(currentPath)}
            title="Refresh"
            className="btn-tactile rounded-md border border-ink-700 bg-ink-800 p-1.5 text-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          </button>
        </div>
      </div>

      {/* Breadcrumb bar */}
      <div className="flex items-center gap-1 border-b border-ink-700 bg-ink-950 px-4 py-1.5 font-mono text-xs text-text-muted overflow-x-auto">
        <button
          type="button"
          onClick={() => void loadDir(DEVICE_ROOT)}
          className="btn-tactile hover:text-text-strong cursor-pointer shrink-0"
        >
          Internal storage
        </button>
        {relativeSegments.map((seg, idx) => {
          const segPath = `${DEVICE_ROOT}/${relativeSegments.slice(0, idx + 1).join("/")}`;
          return (
            <span key={segPath} className="flex items-center gap-1 shrink-0">
              <ChevronRight className="h-3 w-3 text-text-muted" />
              <button
                type="button"
                onClick={() => void loadDir(segPath)}
                className="btn-tactile hover:text-text-strong cursor-pointer"
              >
                {seg}
              </button>
            </span>
          );
        })}
      </div>

      {/* Confirmation / status / error banners */}
      {confirmDelete && (
        <div className="flex items-center justify-between border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger">
          <span>Delete {selected.size} selected item(s) from the phone? This cannot be undone.</span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => setConfirmDelete(false)}
              className="btn-tactile rounded px-2 py-1 text-text hover:bg-ink-800 cursor-pointer"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => void handleDeleteConfirmed()}
              className="btn-tactile rounded bg-status-danger-solid px-2.5 py-1 font-medium text-text-strong hover:opacity-90 cursor-pointer"
            >
              Confirm Delete
            </button>
          </div>
        </div>
      )}

      {error && (
        <div className="border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger">
          {error}
        </div>
      )}
      {statusMsg && !error && (
        <div className="border-b border-status-success/40 bg-status-success-bg px-4 py-1.5 text-xs text-status-success">
          {statusMsg}
        </div>
      )}

      {/* File table + drop target */}
      <div
        className={`relative flex-1 overflow-y-auto ${
          dragOver ? "ring-2 ring-inset ring-gold-400/60 bg-gold-400/10" : ""
        }`}
      >
        <table className="w-full text-left text-xs">
          <thead className="sticky top-0 border-b border-ink-700 bg-ink-900 text-[11px] uppercase tracking-wider text-text-muted">
            <tr>
              <th className="w-8 px-4 py-2">
                <input
                  type="checkbox"
                  checked={entries.length > 0 && selected.size === entries.length}
                  onChange={toggleAll}
                  className="h-3.5 w-3.5 accent-gold-400"
                />
              </th>
              <th className="px-2 py-2 font-medium">Name</th>
              <th className="w-24 px-3 py-2 font-medium text-right">Size</th>
              <th className="w-40 px-4 py-2 font-medium text-right">Modified</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-ink-700">
            {entries.map((entry) => {
              const isSelected = selected.has(entry.path);
              return (
                <tr
                  key={entry.path}
                  onDoubleClick={() => {
                    if (entry.is_dir) void loadDir(entry.path);
                  }}
                  className={`transition-colors duration-150 ${
                    isSelected ? "bg-gold-400/10" : "hover:bg-ink-900"
                  }`}
                >
                  <td className="px-4 py-2">
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() => toggleSelect(entry.path)}
                      className="h-3.5 w-3.5 accent-gold-400"
                    />
                  </td>
                  <td
                    className={`px-2 py-2 flex items-center gap-2 ${
                      entry.is_dir ? "cursor-pointer text-text-strong font-medium" : "text-text"
                    }`}
                    onClick={() => {
                      if (entry.is_dir) void loadDir(entry.path);
                    }}
                  >
                    {entry.is_dir ? (
                      <Folder className="h-4 w-4 shrink-0 text-gold-400" />
                    ) : (
                      <File className="h-4 w-4 shrink-0 text-text-muted" />
                    )}
                    <span className="truncate">{entry.name}</span>
                  </td>
                  <td className="px-3 py-2 text-right font-mono text-text-muted">
                    {entry.is_dir ? "—" : formatBytes(entry.size)}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-text-muted">
                    {formatDate(entry.modified_unix)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        {!loading && entries.length === 0 && (
          <div className="flex h-48 flex-col items-center justify-center text-xs text-text-muted">
            <p>This folder is empty</p>
            <p className="mt-1 text-[11px] text-text-muted">Drop files here to upload</p>
          </div>
        )}
      </div>
    </div>
  );
}
