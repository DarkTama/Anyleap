import { useRef, type KeyboardEvent } from "react";
import { SECTIONS, type SectionId } from "./registry";

/**
 * Left rail (SPEC-28 §10): `nav` landmark with a list of buttons. Roving focus: Tab enters
 * the rail once, Up/Down/Home/End move between sections, Enter/Space opens one.
 * Items are at least 32 px high. Counts are shown only while a filter is active.
 */
export function SettingsRail({
  current,
  visible,
  counts,
  showCounts,
  onOpen,
}: {
  current: SectionId;
  /** Sections to list (all of them when no filter is active). */
  visible: SectionId[];
  counts: Record<SectionId, number>;
  showCounts: boolean;
  onOpen: (id: SectionId) => void;
}) {
  const listRef = useRef<HTMLUListElement>(null);
  const items = SECTIONS.filter((s) => visible.includes(s.id));
  // The roving tab stop is the current section when listed, else the first item.
  const tabStop = items.some((s) => s.id === current) ? current : items[0]?.id;

  const onKeyDown = (e: KeyboardEvent<HTMLUListElement>) => {
    const buttons = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>("button") ?? []);
    if (buttons.length === 0) return;
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    let next = at;
    if (e.key === "ArrowDown") next = at < 0 ? 0 : Math.min(buttons.length - 1, at + 1);
    else if (e.key === "ArrowUp") next = at < 0 ? 0 : Math.max(0, at - 1);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = buttons.length - 1;
    else return;
    e.preventDefault();
    buttons[next]?.focus();
  };

  return (
    <nav aria-label="Settings sections" className="md:sticky md:top-0 md:self-start">
      <ul
        ref={listRef}
        onKeyDown={onKeyDown}
        className="flex gap-1 overflow-x-auto md:w-52 md:flex-col md:overflow-visible"
      >
        {items.map((s) => {
          const isCurrent = s.id === current;
          return (
            <li key={s.id} className="shrink-0">
              <button
                type="button"
                tabIndex={s.id === tabStop ? 0 : -1}
                aria-current={isCurrent ? "page" : undefined}
                onClick={() => onOpen(s.id)}
                className={`flex min-h-8 w-full cursor-pointer items-center justify-between gap-2 rounded-lg px-3 py-1.5 text-left text-xs font-medium whitespace-nowrap focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 ${
                  isCurrent
                    ? "bg-gold-400 text-ink-950"
                    : "bg-ink-900 text-text hover:text-text-strong"
                }`}
              >
                <span>{s.label}</span>
                {showCounts && (
                  <span
                    aria-label={`${counts[s.id]} matches`}
                    className="font-mono text-[10px] font-bold"
                  >
                    {counts[s.id]}
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
