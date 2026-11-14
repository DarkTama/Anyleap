import { useEffect, useMemo, useRef, useState } from "react";
import { Command, Search } from "lucide-react";
import {
  filterAndRankCommands,
  type PaletteCommand,
} from "@/lib/commandPalette";

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  commands: PaletteCommand[];
}

export function CommandPalette({ open, onClose, commands }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const prevFocusRef = useRef<HTMLElement | null>(null);

  const filtered = useMemo(
    () => filterAndRankCommands(commands, query),
    [commands, query],
  );

  useEffect(() => {
    setActiveIndex(0);
  }, [query, open]);

  useEffect(() => {
    if (open) {
      prevFocusRef.current = document.activeElement as HTMLElement | null;
      setQuery("");
      setActiveIndex(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    } else if (prevFocusRef.current) {
      prevFocusRef.current.focus();
      prevFocusRef.current = null;
    }
  }, [open]);

  // Focus trap inside dialog
  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key === "Tab" && dialogRef.current) {
        const focusable = dialogRef.current.querySelectorAll<HTMLElement>(
          'input, button:not([disabled]), [tabindex]:not([tabindex="-1"])',
        );
        if (focusable.length === 0) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  const clampedIndex = filtered.length > 0 ? Math.min(activeIndex, filtered.length - 1) : -1;
  const activeCmd = clampedIndex >= 0 ? filtered[clampedIndex] : undefined;
  const activeDescendantId = activeCmd ? `cmd-option-${activeCmd.id}` : undefined;

  const execute = (cmd: PaletteCommand | undefined) => {
    if (!cmd || cmd.disabled) return;
    onClose();
    void cmd.run();
  };

  const handleInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (filtered.length > 0) {
        setActiveIndex((i) => (i + 1) % filtered.length);
      }
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (filtered.length > 0) {
        setActiveIndex((i) => (i - 1 + filtered.length) % filtered.length);
      }
    } else if (e.key === "Enter") {
      e.preventDefault();
      execute(activeCmd);
    }
  };

  // Keyboard-initiated palette: zero open/close/selection animation per design-engineering rules.
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/65 pt-24 px-4 select-none"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        className="flex w-full max-w-lg flex-col overflow-hidden rounded-xl border border-ink-700 bg-ink-900 text-text shadow-elevated"
      >
        {/* Search Input */}
        <div className="flex items-center gap-2.5 border-b border-ink-700 px-3.5 py-3">
          <Search className="h-4 w-4 shrink-0 text-text-muted" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleInputKeyDown}
            placeholder="Type a command or search..."
            aria-label="Search commands"
            aria-controls="command-palette-list"
            aria-activedescendant={activeDescendantId}
            className="flex-1 bg-transparent text-sm text-text-strong placeholder:text-text-muted focus:outline-none"
          />
          <kbd className="rounded border border-ink-700 bg-ink-950 px-1.5 py-0.5 font-mono text-[10px] text-text-muted">
            ESC
          </kbd>
        </div>

        {/* Command List */}
        <div
          id="command-palette-list"
          role="listbox"
          aria-activedescendant={activeDescendantId}
          className="max-h-80 overflow-y-auto p-1.5"
        >
          {filtered.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-8 text-xs text-text-muted">
              <Command className="mb-2 h-5 w-5 text-text-muted" />
              <span>No matching commands</span>
            </div>
          ) : (
            filtered.map((cmd, idx) => {
              const isSelected = idx === clampedIndex;
              const isDisabled = Boolean(cmd.disabled);
              return (
                <button
                  key={cmd.id}
                  id={`cmd-option-${cmd.id}`}
                  type="button"
                  role="option"
                  aria-selected={isSelected}
                  aria-disabled={isDisabled}
                  onMouseEnter={() => setActiveIndex(idx)}
                  onClick={() => execute(cmd)}
                  className={`flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-left text-xs ${
                    isDisabled
                      ? "opacity-45 cursor-not-allowed"
                      : "cursor-pointer"
                  } ${
                    isSelected
                      ? "bg-ink-800 text-text-strong"
                      : "text-text hover:bg-ink-800/60"
                  }`}
                >
                  <div className="flex items-center gap-2.5 min-w-0">
                    <span
                      className={`rounded px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wider border ${
                        isSelected
                          ? "border-gold-400/50 bg-ink-900 text-gold-400"
                          : "border-ink-700 bg-ink-950 text-text-muted"
                      }`}
                    >
                      {cmd.group}
                    </span>
                    <span className="truncate font-medium">{cmd.label}</span>
                  </div>
                  {cmd.disabled && (
                    <span className="shrink-0 font-mono text-[10px] text-text-muted">
                      {cmd.disabled}
                    </span>
                  )}
                </button>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
