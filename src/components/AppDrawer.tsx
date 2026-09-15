import { useEffect, useState, useMemo } from "react";
import { Loader2, Play, RefreshCw, Rocket, Search, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { listInstalledApps, launchApp } from "@/lib/tauri";

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
      onClose();
    } catch (e) {
      setError(String(e));
      setLaunching(null);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-xs select-none"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="flex h-[480px] w-full max-w-md flex-col rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-100 shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-zinc-800 px-4 py-3">
          <div className="flex items-center gap-2">
            <Rocket className="h-4 w-4 text-sky-400" />
            <h2 className="text-sm font-semibold">Installed Apps</h2>
            {!loading && (
              <span className="rounded-full bg-zinc-800 px-2 py-0.5 text-[10px] text-zinc-400">
                {apps.length}
              </span>
            )}
          </div>
          <div className="flex items-center gap-1">
            <Button
              size="icon"
              variant="ghost"
              className="h-7 w-7 text-zinc-400 hover:text-zinc-200"
              onClick={fetchApps}
              disabled={loading}
              title="Refresh app list"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            </Button>
            <Button
              size="icon"
              variant="ghost"
              className="h-7 w-7 text-zinc-400 hover:text-zinc-200"
              onClick={onClose}
              title="Close"
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>

        {/* Search */}
        <div className="border-b border-zinc-800/80 px-3 py-2">
          <div className="relative flex items-center">
            <Search className="absolute left-2.5 h-3.5 w-3.5 text-zinc-500" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search apps..."
              className="h-8 w-full rounded-md border border-zinc-800 bg-zinc-950 pl-8 pr-8 text-xs text-zinc-200 placeholder:text-zinc-500 focus:border-sky-500 focus:outline-none"
              autoFocus
            />
            {search && (
              <button
                onClick={() => setSearch("")}
                className="absolute right-2.5 text-zinc-500 hover:text-zinc-300"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            )}
          </div>
        </div>

        {/* Error message */}
        {error && (
          <div className="border-b border-red-900/50 bg-red-950/40 px-3 py-1.5 text-xs text-red-300">
            {error}
          </div>
        )}

        {/* List Content */}
        <div className="flex-1 overflow-y-auto p-2">
          {loading ? (
            <div className="flex h-full flex-col items-center justify-center gap-2 text-xs text-zinc-500">
              <Loader2 className="h-5 w-5 animate-spin text-sky-500" />
              <span>Loading packages...</span>
            </div>
          ) : apps.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-xs text-zinc-500">
              No 3rd party apps found.
            </div>
          ) : filtered.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-xs text-zinc-500">
              No apps matching &ldquo;{search}&rdquo;
            </div>
          ) : (
            <div className="space-y-1">
              {filtered.map((pkg) => {
                const isLaunching = launching === pkg;
                return (
                  <button
                    key={pkg}
                    onClick={() => handleLaunch(pkg)}
                    disabled={!!launching}
                    className="flex w-full items-center justify-between rounded-lg px-2.5 py-2 text-left transition hover:bg-zinc-800/80 active:bg-zinc-800 disabled:opacity-50"
                  >
                    <div className="min-w-0 flex-1 pr-2">
                      <div className="truncate text-xs font-medium text-zinc-200">
                        {formatAppName(pkg)}
                      </div>
                      <div className="truncate font-mono text-[10px] text-zinc-500">
                        {pkg}
                      </div>
                    </div>
                    <div className="shrink-0 text-zinc-400">
                      {isLaunching ? (
                        <Loader2 className="h-4 w-4 animate-spin text-sky-400" />
                      ) : (
                        <Play className="h-3.5 w-3.5 text-zinc-500 opacity-0 group-hover:opacity-100 hover:text-sky-400" />
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
