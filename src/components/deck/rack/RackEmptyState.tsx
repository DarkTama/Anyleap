/** Empty state displayed in the device rack when no live or saved devices exist. */
export function RackEmptyState() {
  return (
    <div className="rounded-xl border border-dashed border-ink-700 p-5 text-center">
      <p className="text-xs text-text font-medium">No devices detected</p>
      <p className="mt-1 text-[11px] text-text-muted leading-relaxed">
        Connect via USB with USB Debugging enabled, or click Pair Device.
      </p>
    </div>
  );
}
