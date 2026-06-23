import { Badge } from "@/components/ui/badge";
import type { DeviceState } from "@/lib/types";

const STYLES: Record<string, string> = {
  device: "bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300",
  unauthorized: "bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300",
  offline: "bg-zinc-200 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300",
};

const LABELS: Record<string, string> = {
  device: "ready",
};

const FALLBACK = "bg-zinc-200 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300";

export function DeviceStateBadge({ state }: { state: DeviceState }) {
  return <Badge className={STYLES[state] ?? FALLBACK}>{LABELS[state] ?? state}</Badge>;
}
