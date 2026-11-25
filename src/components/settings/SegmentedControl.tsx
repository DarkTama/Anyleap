import { useId, type KeyboardEvent } from "react";

export interface Segment<T extends string | number> {
  value: T;
  label: string;
}

/**
 * Single-choice segmented control (`radiogroup`). Selected = ink-950 on gold-400
 * (9.30:1); unselected = text on ink-900. A value outside the segments is shown
 * by the optional `customLabel` badge instead of a selected segment.
 */
export function SegmentedControl<T extends string | number>({
  label,
  hint,
  segments,
  value,
  customLabel,
  disabled = false,
  disabledReason,
  onChange,
}: {
  label: string;
  hint?: string;
  segments: Segment<T>[];
  value: T | null;
  customLabel?: string;
  disabled?: boolean;
  /** Shown instead of the hint while the control is disabled. */
  disabledReason?: string;
  onChange: (value: T) => void;
}) {
  const id = useId();
  const helper = disabled && disabledReason ? disabledReason : hint;
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    const keys = ["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp", "Home", "End"];
    if (!keys.includes(e.key)) return;
    e.preventDefault();
    const current = segments.findIndex((s) => s.value === value);
    let next = current < 0 ? 0 : current;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (next + 1) % segments.length;
    if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      next = (next - 1 + segments.length) % segments.length;
    }
    if (e.key === "Home") next = 0;
    if (e.key === "End") next = segments.length - 1;
    onChange(segments[next].value);
    const group = e.currentTarget;
    requestAnimationFrame(() =>
      group.querySelectorAll<HTMLElement>('[role="radio"]')[next]?.focus(),
    );
  };
  const selectedIndex = segments.findIndex((s) => s.value === value);
  return (
    <div className="space-y-1 py-1">
      <div className="flex items-center gap-2">
        <span
          id={`${id}-label`}
          className={`text-sm ${disabled ? "text-text opacity-70" : "text-text-strong"}`}
        >
          {label}
        </span>
        {customLabel && (
          <span className="rounded border border-gold-400 px-1.5 py-0.5 font-mono text-[10px] text-gold-400">
            {customLabel}
          </span>
        )}
      </div>
      <div
        role="radiogroup"
        aria-labelledby={`${id}-label`}
        aria-describedby={helper ? `${id}-hint` : undefined}
        aria-disabled={disabled || undefined}
        onKeyDown={onKeyDown}
        className={`inline-flex overflow-hidden rounded-lg border border-ink-700 ${
          disabled ? "opacity-60" : ""
        }`}
      >
        {segments.map((s, i) => {
          const selected = i === selectedIndex;
          return (
            <button
              key={String(s.value)}
              type="button"
              role="radio"
              aria-checked={selected}
              disabled={disabled}
              tabIndex={selected || (selectedIndex < 0 && i === 0) ? 0 : -1}
              onClick={() => {
                if (!disabled) onChange(s.value);
              }}
              className={`min-h-8 px-3 text-xs font-medium focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-gold-400 ${
                disabled ? "cursor-not-allowed" : "cursor-pointer"
              } ${
                selected ? "bg-gold-400 text-ink-950" : "bg-ink-900 text-text hover:text-text-strong"
              } ${i > 0 ? "border-l border-ink-700" : ""}`}
            >
              {s.label}
            </button>
          );
        })}
      </div>
      {helper && (
        <p id={`${id}-hint`} className="text-[11px] leading-snug text-text-muted">
          {helper}
        </p>
      )}
    </div>
  );
}
