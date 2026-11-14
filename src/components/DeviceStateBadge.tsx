import { Badge } from "@/components/ui/badge";
import type { DeviceState } from "@/lib/types";

const STYLES: Record<string, string> = {
  device: "bg-status-success-bg text-status-success border border-status-success/40",
  unauthorized: "bg-status-warning-bg text-status-warning border border-status-warning/40",
  offline: "bg-ink-800 text-text-muted border border-ink-700",
};

const LABELS: Record<string, string> = {
  device: "ready",
};

const FALLBACK = "bg-ink-800 text-text-muted border border-ink-700";

export function DeviceStateBadge({ state }: { state: DeviceState }) {
  return <Badge className={STYLES[state] ?? FALLBACK}>{LABELS[state] ?? state}</Badge>;
}
