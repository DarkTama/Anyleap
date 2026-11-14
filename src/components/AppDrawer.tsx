import { useEffect, useState, useMemo } from "react";
import { AppWindow, Loader2, Play, RefreshCw, Rocket, Search, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { listInstalledApps, launchApp, launchAppWindow } from "@/lib/tauri";
import { logActivity } from "@/stores/activityStore";

function formatAppName(pkg: string): string {
  const parts = pkg.split(".").filter(Boolean);
  const last = parts[parts.length - 1] || pkg;
  const name =
    (last === "android" || last === "app") && parts.length > 1
      ? parts[parts.length - 2]
      : last;
  return name.charAt(0).toUpperCase() + name.slice(1);
}

export function AppDrawer({
  serial,
  onClose,
}: {
  serial: string;
  onClose: () => void;
}) {
  const [apps, setApps] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [launching, setLaunching] = useState<string | null>(null);

  const fetchApps = async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await listInstalledApps(serial);
      setApps(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchApps();
  }, [serial]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return apps;
    return apps.filter((pkg) => {
      const friendly = formatAppName(pkg).toLowerCase();
      return pkg.toLowerCase().includes(q) || friendly.includes(q);
    });
  }, [apps, search]);

  const handleLaunch = async (pkg: string) => {
    setLaunching(pkg);
    setError(null);
    try {
      await launchApp(serial, pkg);
      logActivity({
        kind: "app",
        title: `Launched ${formatAppName(pkg)}`,
        detail: serial,
      });
      onClose();
    } catch (e) {
      setError(String(e));
      setLaunching(null);
    }
  };

  const handleLaunchWindow = async (pkg: string) => {
    setLaunching(pkg);
    setError(null);
    try {
      await launchAppWindow(serial, pkg);
      logActivity({
        kind: "app",
        title: `Opened ${formatAppName(pkg)} in window`,
        detail: pkg,
      });
      onClose();
    } catch (e) {
      setError(String(e));
      setLaunching(null);
    }
  };

  return (
    <div
      className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-ink-950/80 p-4 select-none"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal-panel flex h-[480px] w-full max-w-md flex-col rounded-xl border border-ink-700 bg-ink-900 text-text shadow-elevated">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink-700 px-4 py-3">
          <div className="flex items-center gap-2">
            <Rocket className="h-4 w-4 text-gold-400" />
            <h2 className="text-sm font-semibold text-text-strong">Installed Apps</h2>
            {!loading && (
              <span className="rounded-full bg-ink-800 border border-ink-700 px-2 py-0.5 text-[10px] text-text-muted">
                {apps.length}
              </span>
            )}
          </div>
          <div className="flex items-center gap-1">
            <Button
              size="icon"
              variant="ghost"
              className="h-7 w-7 text-text-muted hover:text-text-strong"
              onClick={fetchApps}
              disabled={loading}
              title="Refresh app list"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            </Button>
            <Button
              size="icon"
              variant="ghost"
              className="h-7 w-7 text-text-muted hover:text-text-strong"
              onClick={onClose}
              title="Close"
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>

        {/* Search */}
        <div className="border-b border-ink-700 px-3 py-2">
          <div className="relative flex items-center">
            <Search className="absolute left-2.5 h-3.5 w-3.5 text-text-muted" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search apps..."
              className="h-8 w-full rounded-md border border-ink-700 bg-ink-950 pl-8 pr-8 text-xs text-text-strong placeholder:text-text-muted focus:border-gold-400 focus:outline-none"
              autoFocus
            />
            {search && (
              <button
                onClick={() => setSearch("")}
                className="btn-tactile absolute right-2.5 text-text-muted hover:text-text-strong"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>
        </div>

        {/* Error message */}
        {error && (
          <div className="border-b border-status-danger/40 bg-status-danger-bg px-3 py-1.5 text-xs text-status-danger">
            {error}
          </div>
        )}

        {/* List Content */}
        <div className="flex-1 overflow-y-auto p-2">
          {loading ? (
            <div className="flex h-full flex-col items-center justify-center gap-2 text-xs text-text-muted">
              <Loader2 className="h-5 w-5 animate-spin text-gold-400" />
              <span>Loading packages...</span>
            </div>
          ) : apps.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-xs text-text-muted">
              No 3rd party apps found.
            </div>
          ) : filtered.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-xs text-text-muted">
              No apps matching &ldquo;{search}&rdquo;
            </div>
          ) : (
            <div className="space-y-1">
              {filtered.map((pkg) => {
                const isLaunching = launching === pkg;
                return (
                  <div
                    key={pkg}
                    className="group flex w-full items-center justify-between rounded-lg px-2.5 py-2 transition-colors duration-150 hover:bg-ink-800"
                  >
                    <button
                      type="button"
                      onClick={() => handleLaunch(pkg)}
                      disabled={!!launching}
                      className="btn-tactile min-w-0 flex-1 pr-2 text-left disabled:opacity-50"
                      title="Launch on phone"
                    >
                      <div className="truncate text-xs font-medium text-text-strong">
                        {formatAppName(pkg)}
                      </div>
                      <div className="truncate font-mono text-[10px] text-text-muted">
                        {pkg}
                      </div>
                    </button>
                    <div className="flex shrink-0 items-center gap-1.5 text-text-muted">
                      {isLaunching ? (
                        <Loader2 className="h-4 w-4 animate-spin text-gold-400" />
                      ) : (
                        <>
                          <button
                            type="button"
                            onClick={() => handleLaunch(pkg)}
                            disabled={!!launching}
                            className="btn-tactile rounded p-1 text-text-muted hover:bg-ink-700 hover:text-gold-400"
                            title="Launch on phone"
                          >
                            <Play className="h-3.5 w-3.5" />
                          </button>
                          <button
                            type="button"
                            onClick={() => handleLaunchWindow(pkg)}
                            disabled={!!launching}
                            className="btn-tactile flex items-center gap-1 rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 text-[10px] font-medium text-text hover:border-text-muted hover:text-text-strong"
                            title="Open in its own window"
                          >
                            <AppWindow className="h-3 w-3" />
                            <span>Open in window</span>
                          </button>
                        </>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
