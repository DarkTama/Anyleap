import { useEffect, useState } from "react";

/**
 * Tracks mount/exit state so modals and drawers can play a snappy exit
 * transition (faster than enter) before unmounting.
 */
export function usePresence(open: boolean, exitMs = 140): {
  mounted: boolean;
  closing: boolean;
} {
  const [mounted, setMounted] = useState(open);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    if (open) {
      setMounted(true);
      setClosing(false);
      return;
    }
    if (!mounted) return;
    setClosing(true);
    const id = setTimeout(() => {
      setMounted(false);
      setClosing(false);
    }, exitMs);
    return () => clearTimeout(id);
  }, [open, mounted, exitMs]);

  return { mounted, closing };
}
