import { useId, type KeyboardEvent, type ReactNode } from "react";
import type { MirrorMode } from "@/lib/modeSettings";

export const MODE_TAB_LABELS: Record<MirrorMode, string> = {
  mirror: "Mirror",
  flex: "Flex",
  camera: "Camera",
  hifi: "High fidelity",
};

const ORDER: MirrorMode[] = ["mirror", "flex", "camera", "hifi"];

/**
 * Mode tabs (SPEC-28 §10): tablist / tab / tabpanel, Left/Right/Home/End select.
 * Selecting a tab only changes which panel is visible; it never writes settings.
 */
export function ModeTabs({
  active,
  onSelect,
  children,
}: {
  active: MirrorMode;
  onSelect: (mode: MirrorMode) => void;
  children: ReactNode;
}) {
  const id = useId();
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const i = ORDER.indexOf(active);
    let next = i;
    if (e.key === "ArrowRight") next = (i + 1) % ORDER.length;
    else if (e.key === "ArrowLeft") next = (i - 1 + ORDER.length) % ORDER.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = ORDER.length - 1;
    else return;
    e.preventDefault();
    onSelect(ORDER[next]);
    const list = e.currentTarget;
    requestAnimationFrame(() => list.querySelectorAll<HTMLElement>('[role="tab"]')[next]?.focus());
  };
  return (
    <div>
      <div
        role="tablist"
        aria-label="Mirroring mode"
        onKeyDown={onKeyDown}
        className="flex gap-1 border-b border-ink-700"
      >
        {ORDER.map((m) => {
          const selected = m === active;
          return (
            <button
              key={m}
              id={`${id}-tab-${m}`}
              type="button"
              role="tab"
              aria-selected={selected}
              aria-controls={`${id}-panel-${m}`}
              tabIndex={selected ? 0 : -1}
              onClick={() => onSelect(m)}
              className={`min-h-8 cursor-pointer rounded-t-lg px-3 text-xs font-semibold focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-gold-400 ${
                selected ? "bg-gold-400 text-ink-950" : "bg-ink-900 text-text hover:text-text-strong"
              }`}
            >
              {MODE_TAB_LABELS[m]}
            </button>
          );
        })}
      </div>
      <div
        role="tabpanel"
        id={`${id}-panel-${active}`}
        aria-labelledby={`${id}-tab-${active}`}
        className="pt-3"
      >
        {children}
      </div>
    </div>
  );
}
