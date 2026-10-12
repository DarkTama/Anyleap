import { X, Sparkles, CheckCircle2, AlertCircle, RefreshCw, ArrowRight } from "lucide-react";
import ReactMarkdown from "react-markdown";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { stripAssetsSection } from "@/lib/releaseNotes";

export function UpdateModal() {
  const isModalOpen = useAppUpdateStore((s) => s.isModalOpen);
  const closeModal = useAppUpdateStore((s) => s.closeModal);
  const phase = useAppUpdateStore((s) => s.phase);
  const progress = useAppUpdateStore((s) => s.progress);
  const error = useAppUpdateStore((s) => s.error);
  const hasUpdate = useAppUpdateStore((s) => s.hasUpdate);
  const currentVersion = useAppUpdateStore((s) => s.currentVersion);
  const availableVersion = useAppUpdateStore((s) => s.availableVersion);
  const uninstalledReleases = useAppUpdateStore((s) => s.uninstalledReleases);
  const behindCount = useAppUpdateStore((s) => s.behindCount);
  const installUpdate = useAppUpdateStore((s) => s.installUpdate);
  const restartApp = useAppUpdateStore((s) => s.restartApp);

  if (!isModalOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4">
      <div className="relative w-full max-w-lg rounded-2xl border border-[#1e2330] bg-[#12151d] shadow-2xl overflow-hidden flex flex-col max-h-[85vh]">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[#1e2330] px-5 py-4">
          <div className="flex items-center gap-2.5">
            <div className={`grid h-8 w-8 place-items-center rounded-lg ${
              hasUpdate
                ? "bg-cyan-500/10 text-cyan-400 border border-cyan-500/20"
                : "bg-emerald-500/10 text-emerald-400 border border-emerald-500/20"
            }`}>
              {hasUpdate ? <Sparkles className="h-4 w-4" /> : <CheckCircle2 className="h-4 w-4" />}
            </div>
            <div>
              <h2 className="text-sm font-semibold text-zinc-100 flex items-center gap-2">
                {hasUpdate ? "Update Available" : "You're on the Latest Version"}
                {hasUpdate && behindCount > 1 && (
                  <span className="rounded-full bg-cyan-500/20 px-2 py-0.5 text-[10px] font-medium text-cyan-300">
                    {behindCount} versions behind
                  </span>
                )}
              </h2>
              <p className="text-xs text-zinc-400">
                {hasUpdate ? (
                  <>
                    v{currentVersion} <ArrowRight className="inline h-3 w-3 mx-0.5 text-zinc-500" /> v{availableVersion}
                  </>
                ) : (
                  <>AnyLeap v{currentVersion}</>
                )}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={closeModal}
            className="rounded-lg p-1.5 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-200 transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto px-5 py-4 space-y-4">
          {!hasUpdate ? (
            <div className="flex flex-col items-center justify-center py-8 text-center space-y-2.5">
              <div className="grid h-12 w-12 place-items-center rounded-full bg-emerald-500/10 text-emerald-400 border border-emerald-500/20">
                <CheckCircle2 className="h-6 w-6" />
              </div>
              <h3 className="text-sm font-semibold text-zinc-200">AnyLeap is Up to Date</h3>
              <p className="text-xs text-zinc-400 max-w-sm leading-relaxed">
                You are currently running version <span className="font-mono text-zinc-200">v{currentVersion}</span>.
                No new updates are available right now.
              </p>
            </div>
          ) : (
            <>
              <div className="text-xs font-mono font-medium text-zinc-400 uppercase tracking-wider">
                What's New in This Release
              </div>

              {uninstalledReleases.length === 0 ? (
                <p className="text-xs text-zinc-400">Latest version available for download.</p>
              ) : (
                uninstalledReleases.map((rel) => (
                  <div key={rel.id} className="rounded-xl border border-[#1e2330] bg-[#161a24] p-3.5 space-y-1.5">
                    <div className="flex items-center justify-between text-xs">
                      <span className="font-semibold text-cyan-300">{rel.name || rel.tagName}</span>
                      {rel.publishedAt && (
                        <span className="text-[11px] text-zinc-500">
                          {new Date(rel.publishedAt).toLocaleDateString()}
                        </span>
                      )}
                    </div>
                    {stripAssetsSection(rel.body ?? "") ? (
                      <div className="text-xs text-zinc-300 leading-relaxed max-h-48 overflow-y-auto bg-black/30 p-2.5 rounded-lg border border-[#1e2330]/60 space-y-2">
                        <ReactMarkdown
                          components={{
                            h1: (p) => <h4 className="text-xs font-semibold text-zinc-100" {...p} />,
                            h2: (p) => <h4 className="text-xs font-semibold text-zinc-100" {...p} />,
                            h3: (p) => <h5 className="mt-1 text-[11px] font-semibold uppercase tracking-wider text-cyan-300" {...p} />,
                            ul: (p) => <ul className="list-disc space-y-1 pl-4" {...p} />,
                            p: (p) => <p className="text-zinc-300" {...p} />,
                            strong: (p) => <strong className="font-semibold text-zinc-100" {...p} />,
                            code: (p) => <code className="rounded bg-zinc-800 px-1 py-0.5 font-mono text-[11px]" {...p} />,
                            a: ({ href, children }) => (
                              <a
                                href={href}
                                onClick={(e) => {
                                  e.preventDefault();
                                  if (href && /^https:\/\/github\.com\//.test(href)) void openUrl(href);
                                }}
                                className="text-cyan-400 underline-offset-2 hover:underline cursor-pointer"
                              >
                                {children}
                              </a>
                            ),
                          }}
                        >
                          {stripAssetsSection(rel.body ?? "")}
                        </ReactMarkdown>
                      </div>
                    ) : (
                      <p className="text-xs text-zinc-500 italic">No release notes provided.</p>
                    )}
                  </div>
                ))
              )}
            </>
          )}
        </div>

        {/* Footer / Action */}
        <div className="border-t border-[#1e2330] bg-[#0e1118] px-5 py-3.5 space-y-3">
          {error && (
            <div className="flex items-center gap-2 text-xs text-rose-300 bg-rose-950/40 border border-rose-800/50 rounded-lg p-2.5">
              <AlertCircle className="h-4 w-4 shrink-0 text-rose-400" />
              <span>{error}</span>
            </div>
          )}

          {!hasUpdate ? (
            <div className="flex items-center justify-end">
              <button
                type="button"
                onClick={closeModal}
                className="rounded-lg border border-[#1e2330] bg-[#141722] px-4 py-2 text-xs font-medium text-zinc-300 hover:border-zinc-700 hover:bg-[#181c2b] transition-all cursor-pointer"
              >
                Close
              </button>
            </div>
          ) : phase === "downloading" ? (
            <div className="space-y-2">
              <div className="flex items-center justify-between text-xs font-mono text-zinc-300">
                <span className="flex items-center gap-1.5">
                  <RefreshCw className="h-3.5 w-3.5 animate-spin text-cyan-400" />
                  Downloading update payload...
                </span>
                <span>{progress !== null ? `${Math.round(progress * 100)}%` : "in progress"}</span>
              </div>
              <div className="h-2 w-full overflow-hidden rounded-full bg-zinc-800">
                <div
                  className="h-full bg-gradient-to-r from-cyan-500 to-blue-500 transition-all duration-300"
                  style={{ width: `${progress !== null ? Math.round(progress * 100) : 100}%` }}
                />
              </div>
              <p className="text-[11px] text-zinc-500 text-center">
                Updating silently in the background. No installer wizards needed.
              </p>
            </div>
          ) : phase === "ready" ? (
            <div className="flex items-center justify-between gap-3">
              <div className="flex items-center gap-2 text-xs text-emerald-300">
                <CheckCircle2 className="h-4 w-4 text-emerald-400 shrink-0" />
                <span>Update ready! Restart to finish.</span>
              </div>
              <button
                type="button"
                onClick={restartApp}
                className="flex items-center gap-1.5 rounded-lg bg-emerald-500 px-4 py-2 text-xs font-semibold text-zinc-950 hover:bg-emerald-400 active:scale-[0.98] transition-all cursor-pointer shadow-[0_0_12px_rgba(16,185,129,0.3)]"
              >
                <span>Restart AnyLeap</span>
              </button>
            </div>
          ) : (
            <div className="flex items-center justify-end gap-2.5">
              <button
                type="button"
                onClick={closeModal}
                className="rounded-lg border border-[#1e2330] bg-[#141722] px-3.5 py-2 text-xs font-medium text-zinc-300 hover:border-zinc-700 hover:bg-[#181c2b] transition-all"
              >
                Later
              </button>
              <button
                type="button"
                onClick={installUpdate}
                className="flex items-center gap-1.5 rounded-lg border border-cyan-500/60 bg-gradient-to-r from-cyan-600 to-blue-600 px-4 py-2 text-xs font-semibold text-white shadow-[0_0_15px_rgba(6,182,212,0.3)] hover:brightness-110 active:scale-[0.98] transition-all cursor-pointer"
              >
                <Sparkles className="h-3.5 w-3.5" />
                <span>Download & Install</span>
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
