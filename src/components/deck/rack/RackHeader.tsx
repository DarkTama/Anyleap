interface RackHeaderProps {
  deviceCount: number;
}

/** Header bar for the left-hand device rack. */
export function RackHeader({ deviceCount }: RackHeaderProps) {
  return (
    <div className="flex items-center justify-between border-b border-ink-700 px-4 py-3 bg-ink-900">
      <div className="flex items-center gap-2">
        <span className="font-mono text-[10px] font-bold uppercase tracking-wider text-text-muted">
          Device Rack
        </span>
        <span className="rounded bg-ink-800 px-1.5 py-0.5 text-[9px] font-mono text-text border border-ink-700">
          {deviceCount}
        </span>
      </div>
      <span className="font-mono text-[10px] text-text-muted">USB / WI-FI</span>
    </div>
  );
}
