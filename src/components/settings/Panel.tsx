import type { ReactNode } from "react";

/** Plain ink-900 surface for a block of rows inside a section (muted text stays on ink-900). */
export function Panel({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`space-y-1 rounded-xl border border-ink-700 bg-ink-900 p-4 ${className}`}>
      {children}
    </div>
  );
}
