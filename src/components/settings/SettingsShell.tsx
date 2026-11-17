import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { Search, X } from "lucide-react";
import { filterSettings } from "@/lib/settingsFilter";
import { SECTIONS, type SectionId } from "./registry";
import { SettingsRail } from "./SettingsRail";

export type ShowFn = (id: string) => boolean;

export type SectionRender = (ctx: {
  show: ShowFn;
  advancedOpen: boolean;
  filtering: boolean;
}) => ReactNode;

/**
 * Settings layout (SPEC-28 §7, §10): filter box, skip link, left rail and a pane.
 * With no query the pane shows the current section; with a query it shows every section
 * that has matches (matching rows only) and the rail lists just those sections.
 * Filtering never changes values.
 */
export function SettingsShell({ sections }: { sections: Record<SectionId, SectionRender> }) {
  const [query, setQuery] = useState("");
  const [current, setCurrent] = useState<SectionId>("general");
  const [focusTick, setFocusTick] = useState(0);
  const filterId = useId();
  const paneRef = useRef<HTMLDivElement>(null);
  const headingRefs = useRef<Partial<Record<SectionId, HTMLHeadingElement | null>>>({});
  const result = useMemo(() => filterSettings(query), [query]);
  const filtering = result.active;

  const shown: SectionId[] = filtering ? result.sections : [current];
  const show: ShowFn = (id) => result.matches.has(id);

  // Opening a section moves focus to its heading (not on first render).
  useEffect(() => {
    if (focusTick === 0) return;
    headingRefs.current[current]?.focus();
  }, [focusTick, current]);

  const open = (id: SectionId) => {
    setCurrent(id);
    if (filtering) {
      headingRefs.current[id]?.scrollIntoView?.({ block: "start" });
      headingRefs.current[id]?.focus();
    } else {
      setFocusTick((t) => t + 1);
    }
  };

  const status = !filtering
    ? ""
    : result.total === 0
      ? `No settings match “${result.query}”`
      : `${result.total} ${result.total === 1 ? "setting matches" : "settings match"}`;

  return (
    <div className="space-y-3">
      <div role="search" className="max-w-md">
        <label htmlFor={filterId} className="sr-only">
          Filter settings
        </label>
        <div className="relative">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-text-muted"
          />
          <input
            id={filterId}
            type="search"
            value={query}
            placeholder="Filter settings"
            autoComplete="off"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape" && query) {
                e.preventDefault();
                setQuery("");
              }
            }}
            className="min-h-8 w-full rounded-lg border border-ink-700 bg-ink-900 py-1.5 pl-8 pr-8 text-sm text-text-strong placeholder:text-text-muted focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
          />
          {query && (
            <button
              type="button"
              aria-label="Clear filter"
              onClick={() => setQuery("")}
              className="absolute right-1 top-1/2 flex h-6 w-6 -translate-y-1/2 cursor-pointer items-center justify-center rounded text-text hover:text-text-strong focus-visible:outline-2 focus-visible:outline-gold-400"
            >
              <X aria-hidden="true" className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
        <div aria-live="polite" className="min-h-4 pt-1 text-[11px] text-text-muted">
          {status}
        </div>
      </div>

      <a
        href="#settings-pane"
        onClick={(e) => {
          e.preventDefault();
          paneRef.current?.focus();
        }}
        className="sr-only focus:not-sr-only focus:inline-block focus:rounded focus:bg-gold-400 focus:px-2 focus:py-1 focus:text-xs focus:text-ink-950"
      >
        Skip to settings
      </a>

      <div className="flex flex-col gap-4 md:flex-row md:items-start">
        <SettingsRail
          current={current}
          visible={filtering ? result.sections : SECTIONS.map((s) => s.id)}
          counts={result.counts}
          showCounts={filtering}
          onOpen={open}
        />

        <div
          id="settings-pane"
          ref={paneRef}
          tabIndex={-1}
          className="min-w-0 flex-1 space-y-6 focus:outline-none"
        >
          {filtering && result.total === 0 && (
            <div className="rounded-xl border border-ink-700 bg-ink-900 p-4 text-sm text-text">
              <p>No settings match “{result.query}”</p>
              <button
                type="button"
                onClick={() => setQuery("")}
                className="btn-tactile mt-2 min-h-8 cursor-pointer rounded border border-ink-700 bg-ink-800 px-3 text-xs text-text hover:text-text-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
              >
                Clear
              </button>
            </div>
          )}
          {shown.map((id) => {
            const meta = SECTIONS.find((s) => s.id === id)!;
            return (
              <section key={id} aria-labelledby={`settings-h-${id}`} className="space-y-3">
                <h2
                  id={`settings-h-${id}`}
                  tabIndex={-1}
                  ref={(el) => {
                    headingRefs.current[id] = el;
                  }}
                  className="text-base font-semibold text-text-strong focus:outline-none focus-visible:outline-2 focus-visible:outline-gold-400"
                >
                  {meta.label}
                </h2>
                {sections[id]({ show, advancedOpen: result.advancedOpen, filtering })}
              </section>
            );
          })}
        </div>
      </div>
    </div>
  );
}
