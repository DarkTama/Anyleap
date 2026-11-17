import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Ellipsis } from "lucide-react";
import type { ControlItem } from "@/lib/controlBarModel";
import { Tooltip, type TooltipSide } from "./Tooltip";
import { cn } from "@/lib/utils";

export const ICON_BTN =
  "btn-tactile inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-ink-700 bg-ink-900 text-text cursor-pointer hover:bg-ink-800 hover:text-text-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-gold-400 disabled:pointer-events-none disabled:opacity-45";
export const ICON_BTN_PRESSED = "border-gold-400 bg-ink-800 text-gold-400 hover:text-gold-400";
export const ICON_BTN_DANGER = "border-status-danger/50 bg-status-danger-bg text-status-danger hover:text-status-danger";

/** "More" button + popover of secondary controls. Keyboard: Enter/Space/click
 *  opens and focuses the first item, Arrow keys move, Escape (or Tab out)
 *  closes and returns focus to the More button. */
export function MorePopover({
  items,
  icons,
  onActivate,
  side = "left",
}: {
  items: ControlItem[];
  icons: Partial<Record<string, ReactNode>>;
  onActivate: (item: ControlItem) => void;
  side?: TooltipSide;
}) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const panelId = useId();

  useEffect(() => {
    if (!open) return;
    setRect(btnRef.current?.getBoundingClientRect() ?? null);
    panelRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!panelRef.current?.contains(t) && !btnRef.current?.contains(t)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const close = (restoreFocus: boolean) => {
    setOpen(false);
    if (restoreFocus) btnRef.current?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const btns = Array.from(panelRef.current?.querySelectorAll<HTMLButtonElement>("button") ?? []);
    const i = btns.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close(true);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = e.key === "ArrowDown" ? (i + 1) % btns.length : (i - 1 + btns.length) % btns.length;
      btns[next]?.focus();
    } else if (e.key === "Home") {
      e.preventDefault();
      btns[0]?.focus();
    } else if (e.key === "End") {
      e.preventDefault();
      btns[btns.length - 1]?.focus();
    } else if (e.key === "Tab") {
      close(false);
    }
  };

  if (items.length === 0) return null;

  const style: React.CSSProperties = rect
    ? side === "left"
      ? { right: window.innerWidth - rect.left + 8, bottom: window.innerHeight - rect.bottom }
      : { left: rect.left, bottom: window.innerHeight - rect.top + 8 }
    : { visibility: "hidden" };

  return (
    <>
      <Tooltip label="More" side={side}>
        <button
          ref={btnRef}
          type="button"
          aria-label="More"
          aria-haspopup="true"
          aria-expanded={open}
          aria-controls={open ? panelId : undefined}
          className={cn(ICON_BTN, open && ICON_BTN_PRESSED)}
          onClick={() => (open ? close(true) : setOpen(true))}
        >
          <Ellipsis className="h-5 w-5" />
        </button>
      </Tooltip>
      {open && (
        <div
          ref={panelRef}
          id={panelId}
          role="group"
          aria-label="More controls"
          onKeyDown={onKeyDown}
          className="popover-panel fixed z-[90] flex w-60 flex-col gap-0.5 rounded-lg border border-ink-700 bg-ink-900 p-1 shadow-elevated"
          style={{ ...style, "--transform-origin": "bottom right" } as React.CSSProperties}
        >
          {items.map((item) => (
            <button
              key={item.id}
              type="button"
              aria-pressed={item.kind === "toggle" ? !!item.pressed : undefined}
              className={cn(
                "btn-tactile flex w-full items-start gap-2 rounded-md border border-transparent px-2 py-1.5 text-left text-xs text-text cursor-pointer hover:bg-ink-800 hover:text-text-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-gold-400",
                item.pressed && "border-gold-400 text-gold-400 hover:text-gold-400",
              )}
              onClick={() => onActivate(item)}
            >
              <span className="mt-0.5 shrink-0">{icons[item.id]}</span>
              <span className="min-w-0">
                <span className="block font-medium">{item.label}</span>
                {item.hint && <span className="block text-[11px] font-normal text-text-muted">{item.hint}</span>}
              </span>
            </button>
          ))}
        </div>
      )}
    </>
  );
}
