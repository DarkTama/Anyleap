import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Check,
  ExternalLink,
  Grid3x3,
  List,
  RefreshCw,
  Search,
} from "lucide-react";
import { phoneListApps, phoneOpenApp } from "@/services/phoneHubService";
import { describeOpenApp } from "@/lib/openApp";
import { logActivity } from "@/stores/activityStore";
import type { PhoneApp } from "@/types/phone";

interface AppsViewProps {
  serial: string;
}

export function AppsView({ serial }: AppsViewProps) {
  const [apps, setApps] = useState<PhoneApp[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<"user" | "system">("user");
  const [viewMode, setViewMode] = useState<"list" | "grid">("list");
  const [openingPkg, setOpeningPkg] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);

  const loadApps = useCallback(
    async (refresh: boolean) => {
      setLoading(true);
      setError(null);
      try {
        const list = await phoneListApps(serial, refresh);
        const sorted = [...list].sort((a, b) =>
          (a.label || a.package).localeCompare(b.label || b.package),
        );
        setApps(sorted);
        if (refresh) {
          setStatusMsg("App list refreshed from the phone");
        }
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [serial],
  );

  useEffect(() => {
    setQuery("");
    setStatusMsg(null);
    void loadApps(false);
  }, [loadApps]);

  const userCount = useMemo(() => apps.filter((a) => !a.system).length, [apps]);
  const systemCount = useMemo(() => apps.filter((a) => a.system).length, [apps]);

  const scopedApps = useMemo(
    () => apps.filter((a) => a.system === (scope === "system")),
    [apps, scope],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return scopedApps;
    return scopedApps.filter(
      (a) =>
        a.label.toLowerCase().includes(q) || a.package.toLowerCase().includes(q),
    );
  }, [scopedApps, query]);

  async function handleOpen(app: PhoneApp) {
    setOpeningPkg(app.package);
    setError(null);
    const name = app.label || app.package;
    setStatusMsg(`Opening ${name}…`);
    try {
      const res = await phoneOpenApp(serial, app.package);
      const msg = describeOpenApp(name, res?.outcome);
      setStatusMsg(msg);
      logActivity({
        kind: "app",
        title: msg,
        detail: app.package,
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setTimeout(() => {
        setOpeningPkg((prev) => (prev === app.package ? null : prev));
      }, 1200);
    }
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-3 border-b border-ink-700 bg-ink-900 px-4 py-2.5">
        <div className="relative w-[260px]">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-text" />
          <label htmlFor="apps-search" className="sr-only">
            Search apps
          </label>
          <input
            id="apps-search"
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search apps by name or package"
            autoComplete="off"
            className="w-full rounded-lg border border-ink-700 bg-ink-950 py-1.5 pl-8 pr-3 text-xs text-text-strong placeholder:text-text-muted transition-[border-color] duration-150 ease-out focus:border-gold-400 focus:outline-none"
          />
        </div>

        <div
          role="group"
          aria-label="App type"
          className="inline-flex items-center gap-0.5 rounded-lg border border-ink-700 bg-ink-950 p-0.5"
        >
          <button
            type="button"
            aria-pressed={scope === "user"}
            onClick={() => setScope("user")}
            className={`btn-tactile inline-flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
              scope === "user"
                ? "bg-ink-800 text-gold-400"
                : "text-text-muted hover:text-text-strong"
            }`}
          >
            User apps <span className="font-mono">{userCount}</span>
          </button>
          <button
            type="button"
            aria-pressed={scope === "system"}
            onClick={() => setScope("system")}
            className={`btn-tactile inline-flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-medium cursor-pointer ${
              scope === "system"
                ? "bg-ink-800 text-gold-400"
                : "text-text-muted hover:text-text-strong"
            }`}
          >
            System apps <span className="font-mono">{systemCount}</span>
          </button>
        </div>

        <div className="flex-1" />

        <div
          role="group"
          aria-label="Layout"
          className="inline-flex items-center gap-0.5 rounded-lg border border-ink-700 bg-ink-950 p-0.5"
        >
          <button
            type="button"
            aria-pressed={viewMode === "list"}
            onClick={() => setViewMode("list")}
            className={`btn-tactile inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-medium cursor-pointer ${
              viewMode === "list"
                ? "bg-ink-800 text-gold-400"
                : "text-text-muted hover:text-text-strong"
            }`}
          >
            <List className="h-3.5 w-3.5" />
            List
          </button>
          <button
            type="button"
            aria-pressed={viewMode === "grid"}
            onClick={() => setViewMode("grid")}
            className={`btn-tactile inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-medium cursor-pointer ${
              viewMode === "grid"
                ? "bg-ink-800 text-gold-400"
                : "text-text-muted hover:text-text-strong"
            }`}
          >
            <Grid3x3 className="h-3.5 w-3.5" />
            Grid
          </button>
        </div>

        <button
          type="button"
          disabled={loading}
          onClick={() => void loadApps(true)}
          className="btn-tactile inline-flex items-center gap-1.5 rounded-lg border border-ink-700 bg-ink-800 px-2.5 py-1 text-xs font-medium text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          Refresh list
        </button>
      </div>

      {error && (
        <div
          role="alert"
          className="border-b border-status-danger/40 bg-status-danger-bg px-4 py-2 text-xs text-status-danger"
        >
          {error}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-4 pt-2.5">
        <div className="mb-2 flex items-center justify-between text-xs text-text-muted">
          <span role="status">
            {loading
              ? "Loading apps…"
              : filtered.length > 0
                ? `Showing ${filtered.length} of ${scopedApps.length} ${scope} apps`
                : ""}
          </span>
          {statusMsg && <span className="text-gold-400">{statusMsg}</span>}
        </div>

        {!loading && filtered.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-ink-700 p-8 text-center">
            <Search className="mb-2 h-10 w-10 text-ink-700" />
            <h2 className="text-sm font-semibold text-text-strong">
              {query.trim()
                ? `No apps match “${query.trim()}”`
                : `No ${scope} apps found`}
            </h2>
            <p className="mt-1 max-w-md text-xs text-text">
              {query.trim()
                ? "Check the spelling, or switch between User and System apps."
                : "Try refreshing the app list from the phone."}
            </p>
            {query.trim() && (
              <button
                type="button"
                onClick={() => setQuery("")}
                className="btn-tactile mt-3 rounded-lg border border-ink-700 bg-ink-800 px-3 py-1.5 text-xs font-medium text-text-strong hover:border-text-muted cursor-pointer"
              >
                Clear search
              </button>
            )}
          </div>
        ) : viewMode === "list" ? (
          <ul className="overflow-hidden rounded-xl border border-ink-700 bg-ink-900 divide-y divide-ink-700">
            {filtered.map((app) => {
              const name = app.label || app.package;
              const isOpening = openingPkg === app.package;
              return (
                <li
                  key={app.package}
                  className="flex items-center gap-3 px-3.5 py-2"
                >
                  <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-ink-700 bg-ink-800 text-[13px] font-bold text-text-strong">
                    {(name[0] || "?").toUpperCase()}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-xs font-semibold text-text-strong">
                      {name}
                    </span>
                    <span className="block truncate font-mono text-[11px] text-text-muted">
                      {app.package}
                    </span>
                  </span>
                  {app.system && (
                    <span className="rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 font-mono text-[11px] text-text">
                      System
                    </span>
                  )}
                  <button
                    type="button"
                    disabled={isOpening}
                    aria-label={`Open ${name}`}
                    onClick={() => void handleOpen(app)}
                    className="btn-tactile inline-flex shrink-0 items-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] font-medium text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
                  >
                    {isOpening ? (
                      <>
                        <Check className="h-3 w-3 text-status-success" />
                        Opening…
                      </>
                    ) : (
                      <>
                        <ExternalLink className="h-3 w-3" />
                        Open
                      </>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <ul className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-3">
            {filtered.map((app) => {
              const name = app.label || app.package;
              const isOpening = openingPkg === app.package;
              return (
                <li
                  key={app.package}
                  className="flex min-w-0 flex-col items-start gap-2 rounded-xl border border-ink-700 bg-ink-900 p-3"
                >
                  <div className="flex w-full items-center justify-between gap-2">
                    <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-ink-700 bg-ink-800 text-[13px] font-bold text-text-strong">
                      {(name[0] || "?").toUpperCase()}
                    </span>
                    {app.system && (
                      <span className="rounded border border-ink-700 bg-ink-800 px-1.5 py-0.5 font-mono text-[11px] text-text">
                        System
                      </span>
                    )}
                  </div>
                  <span className="w-full truncate text-xs font-semibold text-text-strong">
                    {name}
                  </span>
                  <span className="w-full truncate font-mono text-[11px] text-text-muted">
                    {app.package}
                  </span>
                  <button
                    type="button"
                    disabled={isOpening}
                    aria-label={`Open ${name}`}
                    onClick={() => void handleOpen(app)}
                    className="btn-tactile inline-flex w-full items-center justify-center gap-1 rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-[11px] font-medium text-text hover:border-text-muted hover:text-text-strong disabled:opacity-50 cursor-pointer"
                  >
                    {isOpening ? (
                      <>
                        <Check className="h-3 w-3 text-status-success" />
                        Opening…
                      </>
                    ) : (
                      <>
                        <ExternalLink className="h-3 w-3" />
                        Open
                      </>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
