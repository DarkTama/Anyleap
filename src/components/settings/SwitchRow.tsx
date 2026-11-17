import { useId } from "react";

/**
 * Labelled switch (SPEC-28 §10): visible label, `role="switch"`, helper linked with
 * `aria-describedby`. State is shown by thumb position as well as colour.
 * Helper text is `text-muted`, so rows must sit on `ink-900` / `ink-950` only.
 */
export function SwitchRow({
  label,
  hint,
  checked,
  disabled = false,
  disabledReason,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  disabled?: boolean;
  /** Shown instead of the hint while the row is disabled. */
  disabledReason?: string;
  onChange: (value: boolean) => void;
}) {
  const id = useId();
  const helper = disabled && disabledReason ? disabledReason : hint;
  return (
    <div className="flex min-h-8 items-start justify-between gap-3 py-1">
      <div className="min-w-0">
        <label
          id={`${id}-label`}
          htmlFor={`${id}-switch`}
          className={`text-sm ${disabled ? "text-text opacity-70" : "text-text-strong"}`}
        >
          {label}
        </label>
        {helper && (
          <p id={`${id}-hint`} className="text-[11px] leading-snug text-text-muted">
            {helper}
          </p>
        )}
      </div>
      <button
        id={`${id}-switch`}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-labelledby={`${id}-label`}
        aria-describedby={helper ? `${id}-hint` : undefined}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative mt-0.5 inline-flex h-5 w-9 shrink-0 items-center rounded-full border border-ink-700 transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-gold-400 ${
          checked ? "bg-gold-400" : "bg-ink-950"
        } ${disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer"}`}
      >
        <span
          aria-hidden="true"
          className={`inline-block h-3.5 w-3.5 rounded-full transition-transform duration-150 ease-out motion-reduce:transition-none ${
            checked ? "translate-x-[18px] bg-ink-950" : "translate-x-[3px] bg-text"
          }`}
        />
      </button>
    </div>
  );
}
