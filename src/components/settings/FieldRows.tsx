import { useId } from "react";
import { selectClass } from "@/lib/styles";

/** Number input with a visible bound label and an `aria-describedby` helper. */
export function NumberField({
  label,
  hint,
  value,
  min = 0,
  onChange,
}: {
  label: string;
  hint?: string;
  value: number;
  min?: number;
  onChange: (v: number) => void;
}) {
  const id = useId();
  return (
    <div className="space-y-1 py-1">
      <label htmlFor={id} className="text-sm text-text-strong">
        {label}
      </label>
      <input
        id={id}
        type="number"
        min={min}
        aria-describedby={hint ? `${id}-hint` : undefined}
        className={selectClass}
        value={value}
        onChange={(e) => onChange(Number(e.target.value) || 0)}
      />
      {hint && (
        <p id={`${id}-hint`} className="text-[11px] text-text-muted">
          {hint}
        </p>
      )}
    </div>
  );
}

/** Select with a visible bound label and helper. */
export function SelectField({
  label,
  hint,
  value,
  options,
  onChange,
}: {
  label: string;
  hint?: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (v: string) => void;
}) {
  const id = useId();
  return (
    <div className="space-y-1 py-1">
      <label htmlFor={id} className="text-sm text-text-strong">
        {label}
      </label>
      <select
        id={id}
        aria-describedby={hint ? `${id}-hint` : undefined}
        className={selectClass}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value} className="bg-ink-900 text-text">
            {o.label}
          </option>
        ))}
      </select>
      {hint && (
        <p id={`${id}-hint`} className="text-[11px] text-text-muted">
          {hint}
        </p>
      )}
    </div>
  );
}

/** Read-only value with a link to the editor (SPEC-28 §9.1). */
export function ReadOnlyLinkRow({
  label,
  hint,
  value,
  linkLabel,
  onLink,
}: {
  label: string;
  hint?: string;
  value: string;
  linkLabel: string;
  onLink?: () => void;
}) {
  const id = useId();
  return (
    <div className="flex items-start justify-between gap-3 py-1">
      <div className="min-w-0">
        <div id={`${id}-label`} className="text-sm text-text-strong">
          {label}
        </div>
        {hint && (
          <p id={`${id}-hint`} className="text-[11px] leading-snug text-text-muted">
            {hint}
          </p>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-3">
        <output
          aria-labelledby={`${id}-label`}
          aria-describedby={hint ? `${id}-hint` : undefined}
          className="font-mono text-sm font-semibold text-gold-400"
        >
          {value}
        </output>
        <button
          type="button"
          onClick={onLink}
          className="min-h-8 cursor-pointer text-xs font-medium text-gold-400 underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400"
        >
          {linkLabel}
        </button>
      </div>
    </div>
  );
}
