import { useEffect, useId, useRef, useState } from "react";
import { Mouse } from "lucide-react";
import { useAppStore } from "@/store/useAppStore";
import { editMode, openPhoneMirroringSettings } from "@/lib/modeActions";
import {
  SCROLL_SPEED_OPTIONS,
  TOUCHPAD_SPEED_OPTIONS,
  formatScrollMultiplier,
  type DisplayMode,
  type ScrollSettings,
  type ScrollStyle,
} from "@/lib/modeSettings";
import { selectClass, optionClass } from "@/lib/styles";
import { getScrollMultiplier } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { ICON_BTN, ICON_BTN_PRESSED } from "./MorePopover";
import { MODE_TAB_LABELS } from "./settings/ModeTabs";
import { SegmentedControl } from "./settings/SegmentedControl";
import { SwitchRow } from "./settings/SwitchRow";
import { Tooltip, type TooltipSide } from "./Tooltip";

const SCROLL_STYLE_SEGMENTS: { value: ScrollStyle; label: string }[] = [
  { value: "smooth", label: "Smooth (reading)" },
  { value: "paged", label: "Paged (short videos)" },
];

function focusableElements(container: HTMLElement): HTMLElement[] {
  return Array.from(
    container.querySelectorAll<HTMLElement>(
      'button:not([disabled]):not([tabindex="-1"]), select:not([disabled]), input:not([disabled]), a[href], [tabindex]:not([tabindex="-1"]):not([disabled])',
    ),
  );
}

/**
 * Control-bar "Scroll" button + popover (SPEC-30 addendum).
 * Edits the running mode's `modeDefaults[mode].scroll` via `editMode` (single source
 * of truth shared with Settings -> Phone mirroring). Keyboard: focus trap,
 * Escape closes and restores focus, arrow keys move within Scroll style segments.
 */
export function ScrollPopover({
  serial,
  mode,
  side = "left",
  onOpenSettings,
}: {
  serial: string;
  mode: DisplayMode;
  side?: TooltipSide;
  onOpenSettings?: (mode: DisplayMode) => void;
}) {
  const [open, setOpen] = useState(false);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const [liveMultiplier, setLiveMultiplier] = useState<number | null>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const panelId = useId();

  const scroll = useAppStore((st) => st.modeDefaults[mode].scroll);
  const isPaged = scroll.style === "paged";
  const isAuto = scroll.speed === "auto";

  const updateScroll = (patch: Partial<ScrollSettings>) => {
    editMode(mode, { scroll: { ...scroll, ...patch } });
  };

  useEffect(() => {
    if (!open) return;
    setRect(btnRef.current?.getBoundingClientRect() ?? null);
    if (panelRef.current) {
      focusableElements(panelRef.current)[0]?.focus();
    }
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!panelRef.current?.contains(t) && !btnRef.current?.contains(t)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  useEffect(() => {
    if (!open || !serial || isPaged || !isAuto) {
      setLiveMultiplier(null);
      return;
    }
    let cancelled = false;
    const refresh = () => {
      getScrollMultiplier(serial)
        .then((m) => {
          if (!cancelled) setLiveMultiplier(m);
        })
        .catch(() => {
          if (!cancelled) setLiveMultiplier(null);
        });
    };
    refresh();
    const timer = setInterval(refresh, 2000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [open, serial, isPaged, isAuto]);

  const close = (restoreFocus: boolean) => {
    setOpen(false);
    if (restoreFocus) btnRef.current?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close(true);
      return;
    }
    if (e.key === "Tab" && panelRef.current) {
      const els = focusableElements(panelRef.current);
      if (els.length === 0) return;
      const first = els[0];
      const last = els[els.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (e.shiftKey) {
        if (active === first || !panelRef.current.contains(active)) {
          e.preventDefault();
          last.focus();
        }
      } else if (active === last || !panelRef.current.contains(active)) {
        e.preventDefault();
        first.focus();
      }
    }
  };

  const style: React.CSSProperties = rect
    ? side === "left"
      ? {
          right: Math.max(8, window.innerWidth - rect.left + 8),
          bottom: Math.max(8, window.innerHeight - rect.bottom),
          maxHeight: "calc(100vh - 16px)",
        }
      : {
          left: Math.max(8, rect.left),
          bottom: Math.max(8, window.innerHeight - rect.top + 8),
          maxHeight: "calc(100vh - 16px)",
        }
    : { visibility: "hidden" };

  const speedId = `${panelId}-speed`;
  const touchpadId = `${panelId}-touchpad-speed`;
  const pagedNoteId = `${panelId}-paged-note`;
  const autoLabel =
    isAuto && liveMultiplier != null ? `Auto · ${formatScrollMultiplier(liveMultiplier)}x` : "Auto";

  return (
    <>
      <Tooltip label="Scroll" side={side}>
        <button
          ref={btnRef}
          type="button"
          aria-label="Scroll"
          aria-haspopup="dialog"
          aria-expanded={open}
          aria-controls={open ? panelId : undefined}
          className={cn(ICON_BTN, open && ICON_BTN_PRESSED)}
          onClick={() => (open ? close(true) : setOpen(true))}
        >
          <Mouse className="h-5 w-5" aria-hidden="true" />
        </button>
      </Tooltip>
      {open && (
        <div
          ref={panelRef}
          id={panelId}
          role="dialog"
          aria-modal="true"
          aria-label="Scroll settings"
          onKeyDown={onKeyDown}
          className="popover-panel fixed z-[90] w-[296px] space-y-2.5 overflow-y-auto rounded-lg border border-ink-700 bg-ink-900 p-3 text-left shadow-elevated"
          style={{ ...style, "--transform-origin": "bottom right" } as React.CSSProperties}
        >
          <SegmentedControl<ScrollStyle>
            label="Scroll style"
            segments={SCROLL_STYLE_SEGMENTS}
            value={scroll.style}
            onChange={(nextStyle) => updateScroll({ style: nextStyle })}
          />

          <div className="space-y-1">
            <label
              htmlFor={speedId}
              className={`block text-sm ${isPaged ? "text-text opacity-70" : "text-text-strong"}`}
            >
              Scroll speed
            </label>
            <select
              id={speedId}
              disabled={isPaged}
              aria-describedby={isPaged ? pagedNoteId : undefined}
              className={`${selectClass} disabled:cursor-not-allowed disabled:opacity-60`}
              value={isAuto ? "auto" : String(scroll.speed)}
              onChange={(e) => {
                const val = e.target.value;
                const speed = val === "auto" ? "auto" : Number.parseFloat(val);
                updateScroll({ speed });
              }}
            >
              <option value="auto" className={optionClass}>
                {autoLabel}
              </option>
              {SCROLL_SPEED_OPTIONS.map((v) => (
                <option key={v} value={String(v)} className={optionClass}>
                  {v}x
                </option>
              ))}
            </select>
          </div>

          <SwitchRow
            label="Smooth scrolling"
            checked={scroll.smooth}
            disabled={isPaged}
            describedById={isPaged ? pagedNoteId : undefined}
            onChange={(smooth) => updateScroll({ smooth })}
          />

          <div className="space-y-1">
            <label
              htmlFor={touchpadId}
              className={`block text-sm ${isPaged ? "text-text opacity-70" : "text-text-strong"}`}
            >
              Touchpad speed
            </label>
            <select
              id={touchpadId}
              disabled={isPaged}
              aria-describedby={isPaged ? pagedNoteId : undefined}
              className={`${selectClass} disabled:cursor-not-allowed disabled:opacity-60`}
              value={String(scroll.touchpadSpeed)}
              onChange={(e) => {
                const touchpadSpeed = Number.parseFloat(e.target.value);
                updateScroll({ touchpadSpeed });
              }}
            >
              {TOUCHPAD_SPEED_OPTIONS.map((v) => (
                <option key={v} value={String(v)} className={optionClass}>
                  {v}x
                </option>
              ))}
            </select>
          </div>

          {isPaged && (
            <p id={pagedNoteId} className="text-[11px] leading-snug text-text-muted">
              Not used in Paged style.
            </p>
          )}

          <div className="flex items-center justify-between border-t border-ink-700 pt-2 text-[11px]">
            <span className="text-text-muted">Saved for {MODE_TAB_LABELS[mode]}</span>
            <button
              type="button"
              onClick={() => {
                close(false);
                if (onOpenSettings) onOpenSettings(mode);
                else openPhoneMirroringSettings(mode);
              }}
              className="btn-tactile cursor-pointer rounded px-1 py-0.5 font-medium text-gold-400 underline hover:text-gold-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-gold-400"
            >
              All settings
            </button>
          </div>
        </div>
      )}
    </>
  );
}
