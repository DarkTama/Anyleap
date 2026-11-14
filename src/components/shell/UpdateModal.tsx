import { X, Sparkles, CheckCircle2, AlertCircle, RefreshCw, ArrowRight } from "lucide-react";
import ReactMarkdown from "react-markdown";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppUpdateStore } from "@/stores/appUpdateStore";
import { stripAssetsSection } from "@/lib/releaseNotes";
import { usePresence } from "@/lib/usePresence";

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

  const { mounted, closing } = usePresence(isModalOpen, 140);
  if (!mounted) return null;

  const state = closing ? "closing" : "open";

  return (
    <div
      data-state={state}
      className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) closeModal();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Application update"
        data-state={state}
        className="modal-panel relative w-full max-w-lg rounded-2xl border border-ink-700 bg-ink-900 shadow-elevated overflow-hidden flex flex-col max-h-[85vh]"
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-ink-700 px-5 py-4">
          <div className="flex items-center gap-2.5">
            <div
              className={`grid h-8 w-8 place-items-center rounded-lg border ${
                hasUpdate
                  ? "bg-ink-800 text-gold-400 border-gold-400/40"
                  : "bg-status-success-bg text-status-success border-status-success/40"
              }`}
            >
              {hasUpdate ? <Sparkles className="h-4 w-4" /> : <CheckCircle2 className="h-4 w-4" />}
            </div>
            <div>
              <h2 className="text-sm font-semibold text-text-strong flex items-center gap-2">
                {hasUpdate ? "Update Available" : "You're on the Latest Version"}
                {hasUpdate && behindCount > 1 && (
                  <span className="rounded-full border border-gold-400/40 bg-ink-800 px-2 py-0.5 text-[10px] font-medium text-gold-400">
                    {behindCount} versions behind
                  </span>
                )}
              </h2>
              <p className="text-xs text-text-muted">
                {hasUpdate ? (
                  <>
                    v{currentVersion} <ArrowRight className="inline h-3 w-3 mx-0.5 text-text-muted" /> v{availableVersion}
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
            className="btn-tactile rounded-lg p-1.5 text-text-muted hover:bg-ink-800 hover:text-text-strong cursor-pointer"
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto px-5 py-4 space-y-4">
          {!hasUpdate ? (
            <div className="flex flex-col items-center justify-center py-8 text-center space-y-2.5">
              <div className="grid h-12 w-12 place-items-center rounded-full bg-status-success-bg text-status-success border border-status-success/40">
                <CheckCircle2 className="h-6 w-6" />
              </div>
              <h3 className="text-sm font-semibold text-text-strong">AnyLeap is Up to Date</h3>
              <p className="text-xs text-text-muted max-w-sm leading-relaxed">
                You are currently running version <span className="font-mono text-text-strong">v{currentVersion}</span>.
                No new updates are available right now.
              </p>
            </div>
          ) : (
            <>
              <div className="text-xs font-mono font-medium text-text-muted uppercase tracking-wider">
                What's New in This Release
              </div>

              {uninstalledReleases.length === 0 ? (
                <p className="text-xs text-text-muted">Latest version available for download.</p>
              ) : (
                uninstalledReleases.map((rel) => (
                  <div key={rel.id} className="rounded-xl border border-ink-700 bg-ink-800 p-3.5 space-y-1.5">
                    <div className="flex items-center justify-between text-xs">
                      <span className="font-semibold text-gold-400">{rel.name || rel.tagName}</span>
                      {rel.publishedAt && (
                        <span className="text-[11px] text-text-muted">
                          {new Date(rel.publishedAt).toLocaleDateString()}
                        </span>
                      )}
                    </div>
                    {stripAssetsSection(rel.body ?? "") ? (
                      <div className="text-xs text-text leading-relaxed max-h-48 overflow-y-auto bg-ink-950 p-2.5 rounded-lg border border-ink-700 space-y-2">
                        <ReactMarkdown
                          components={{
                            h1: (p) => <h4 className="text-xs font-semibold text-text-strong" {...p} />,
                            h2: (p) => <h4 className="text-xs font-semibold text-text-strong" {...p} />,
                            h3: (p) => <h5 className="mt-1 text-[11px] font-semibold uppercase tracking-wider text-mist-300" {...p} />,
                            ul: (p) => <ul className="list-disc space-y-1 pl-4" {...p} />,
                            p: (p) => <p className="text-text" {...p} />,
                            strong: (p) => <strong className="font-semibold text-text-strong" {...p} />,
                            code: (p) => <code className="rounded bg-ink-800 px-1 py-0.5 font-mono text-[11px]" {...p} />,
                            a: ({ href, children }) => (
                              <a
                                href={href}
                                onClick={(e) => {
                                  e.preventDefault();
                                  if (href && /^https:\/\/github\.com\//.test(href)) void openUrl(href);
                                }}
                                className="text-mist-300 underline-offset-2 hover:underline cursor-pointer"
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
                      <p className="text-xs text-text-muted italic">No release notes provided.</p>
                    )}
                  </div>
                ))
              )}
            </>
          )}
        </div>

        {/* Footer / Action */}
        <div className="border-t border-ink-700 bg-ink-950 px-5 py-3.5 space-y-3">
          {error && (
            <div className="flex items-center gap-2 text-xs text-status-danger bg-status-danger-bg border border-status-danger/40 rounded-lg p-2.5">
              <AlertCircle className="h-4 w-4 shrink-0 text-status-danger" />
              <span>{error}</span>
            </div>
          )}

          {!hasUpdate ? (
            <div className="flex items-center justify-end">
              <button
                type="button"
                onClick={closeModal}
                className="btn-tactile rounded-lg border border-ink-700 bg-ink-900 px-4 py-2 text-xs font-medium text-text hover:border-text-muted hover:bg-ink-800 cursor-pointer"
              >
                Close
              </button>
            </div>
          ) : phase === "downloading" ? (
            <div className="space-y-2">
              <div className="flex items-center justify-between text-xs font-mono text-text">
                <span className="flex items-center gap-1.5">
                  <RefreshCw className="h-3.5 w-3.5 animate-spin text-gold-400" />
                  Downloading update payload...
                </span>
                <span>{progress !== null ? `${Math.round(progress * 100)}%` : "in progress"}</span>
              </div>
              <div className="h-2 w-full overflow-hidden rounded-full bg-ink-800">
                <div
                  className="h-full bg-gold-400 transition-[width] duration-200 ease-out"
                  style={{ width: `${progress !== null ? Math.round(progress * 100) : 100}%` }}
                />
              </div>
              <p className="text-[11px] text-text-muted text-center">
                Updating silently in the background. No installer wizards needed.
              </p>
            </div>
          ) : phase === "ready" ? (
            <div className="flex items-center justify-between gap-3">
              <div className="flex items-center gap-2 text-xs text-status-success">
                <CheckCircle2 className="h-4 w-4 text-status-success shrink-0" />
                <span>Update ready! Restart to finish.</span>
              </div>
              <button
                type="button"
                onClick={restartApp}
                className="btn-tactile flex items-center gap-1.5 rounded-lg bg-status-success px-4 py-2 text-xs font-semibold text-ink-950 hover:opacity-90 cursor-pointer"
              >
                <span>Restart AnyLeap</span>
              </button>
            </div>
          ) : (
            <div className="flex items-center justify-end gap-2.5">
              <button
                type="button"
                onClick={closeModal}
                className="btn-tactile rounded-lg border border-ink-700 bg-ink-900 px-3.5 py-2 text-xs font-medium text-text hover:border-text-muted hover:bg-ink-800 cursor-pointer"
              >
                Later
              </button>
              <button
                type="button"
                onClick={installUpdate}
                className="btn-tactile flex items-center gap-1.5 rounded-lg bg-gold-400 px-4 py-2 text-xs font-semibold text-ink-950 hover:bg-gold-600 cursor-pointer"
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
