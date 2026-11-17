import { useCallback, useEffect, useId, useRef, useState, type ReactElement } from "react";

export type TooltipSide = "left" | "right" | "top" | "bottom";

/** Styled tooltip shown on hover and on keyboard focus of its (single) child.
 *  Escape dismisses it. The child keeps its own aria-label; the tooltip text is
 *  also linked through aria-describedby when a hint is present. */
export function Tooltip({
  label,
  hint,
  side = "left",
  children,
}: {
  label: string;
  hint?: string;
  side?: TooltipSide;
  children: ReactElement;
}) {
  const wrapRef = useRef<HTMLSpanElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const id = useId();

  const show = useCallback(() => {
    const el = wrapRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const gap = 8;
    if (side === "left") setPos({ x: r.left - gap, y: r.top + r.height / 2 });
    else if (side === "right") setPos({ x: r.right + gap, y: r.top + r.height / 2 });
    else if (side === "top") setPos({ x: r.left + r.width / 2, y: r.top - gap });
    else setPos({ x: r.left + r.width / 2, y: r.bottom + gap });
  }, [side]);
  const hide = useCallback(() => setPos(null), []);

  useEffect(() => {
    if (!pos) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPos(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pos]);

  const transform: Record<TooltipSide, string> = {
    left: "translate(-100%, -50%)",
    right: "translate(0, -50%)",
    top: "translate(-50%, -100%)",
    bottom: "translate(-50%, 0)",
  };

  return (
    <span
      ref={wrapRef}
      className="inline-flex"
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
      aria-describedby={hint && pos ? id : undefined}
    >
      {children}
      {pos && (
        <span
          id={id}
          role="tooltip"
          className="pointer-events-none fixed z-[100] max-w-[220px] rounded-md border border-ink-700 bg-ink-800 px-2 py-1 text-xs font-medium text-text-strong shadow-elevated"
          style={{ left: pos.x, top: pos.y, transform: transform[side] }}
        >
          {label}
          {hint && <span className="mt-0.5 block text-[11px] font-normal text-text-muted">{hint}</span>}
        </span>
      )}
    </span>
  );
}
