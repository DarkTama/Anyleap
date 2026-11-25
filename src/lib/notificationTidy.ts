import type { PhoneHubNotification } from "@/types/phone";

const SHORT_MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
] as const;

const CLAMP_CHAR_THRESHOLD = 120;

export function collapseNotificationWhitespace(text: string): string {
  if (!text) return "";
  return text
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .map((line) => line.replace(/[ \t]+/g, " ").trim())
    .filter((line) => line.length > 0)
    .join("\n");
}

export const normalizeNotificationText = collapseNotificationWhitespace;

export function shouldShowNotificationExpand(rawText: string, collapsedText: string): boolean {
  if (!collapsedText) return false;
  const normalizedNewlines = rawText.replace(/\r\n?/g, "\n");
  const hasCollapsedBlankLines = /\n[ \t]*\n/.test(normalizedNewlines);
  const lineCount = collapsedText.split("\n").length;
  return hasCollapsedBlankLines || lineCount > 2 || collapsedText.length > CLAMP_CHAR_THRESHOLD;
}

export function normalizeNotificationTitle(title: string): string {
  return title.trim().replace(/\s+/g, " ").toLowerCase();
}

export interface NotificationStack {
  id: string;
  package: string;
  normalizedTitle: string;
  latest: PhoneHubNotification;
  olderItems: PhoneHubNotification[];
}

export function stackSameTitleNotifications(
  items: PhoneHubNotification[],
): NotificationStack[] {
  const sorted = [...items].sort((a, b) => b.postedUnix - a.postedUnix);
  const stacks: NotificationStack[] = [];
  const indexByCluster = new Map<string, number>();

  for (const item of sorted) {
    const normTitle = normalizeNotificationTitle(item.title || "");
    const clusterKey = `${item.package}::${normTitle}`;
    const existingIdx = indexByCluster.get(clusterKey);
    if (existingIdx !== undefined) {
      stacks[existingIdx].olderItems.push(item);
    } else {
      indexByCluster.set(clusterKey, stacks.length);
      stacks.push({
        id: `${clusterKey}::${item.key}::${item.postedUnix}`,
        package: item.package,
        normalizedTitle: normTitle,
        latest: item,
        olderItems: [],
      });
    }
  }

  return stacks;
}

export type DayBucketId = "today" | "yesterday" | "earlier";

export interface DayBucket<T = PhoneHubNotification> {
  id: DayBucketId;
  label: "Today" | "Yesterday" | "Earlier";
  items: T[];
}

function getLocalDayBoundaries(nowMs: number): { todayStart: number; yesterdayStart: number } {
  const now = new Date(nowMs);
  const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const yesterdayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1).getTime();
  return { todayStart, yesterdayStart };
}

export function bucketByLocalDay(
  items: PhoneHubNotification[],
  nowMs = Date.now(),
): DayBucket<PhoneHubNotification>[] {
  const { todayStart, yesterdayStart } = getLocalDayBoundaries(nowMs);
  const sorted = [...items].sort((a, b) => b.postedUnix - a.postedUnix);

  const today: PhoneHubNotification[] = [];
  const yesterday: PhoneHubNotification[] = [];
  const earlier: PhoneHubNotification[] = [];

  for (const item of sorted) {
    const tsMs = item.postedUnix * 1000;
    if (item.postedUnix > 0 && tsMs >= todayStart) {
      today.push(item);
    } else if (item.postedUnix > 0 && tsMs >= yesterdayStart) {
      yesterday.push(item);
    } else {
      earlier.push(item);
    }
  }

  const buckets: DayBucket<PhoneHubNotification>[] = [];
  if (today.length > 0) buckets.push({ id: "today", label: "Today", items: today });
  if (yesterday.length > 0) {
    buckets.push({ id: "yesterday", label: "Yesterday", items: yesterday });
  }
  if (earlier.length > 0) buckets.push({ id: "earlier", label: "Earlier", items: earlier });
  return buckets;
}

function formatHm(date: Date): string {
  const hh = String(date.getHours()).padStart(2, "0");
  const mm = String(date.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

export function formatRelativeNotificationTime(
  postedUnix: number,
  nowMs = Date.now(),
): string {
  if (!postedUnix || postedUnix <= 0) return "";
  const tsMs = postedUnix * 1000;
  const { todayStart, yesterdayStart } = getLocalDayBoundaries(nowMs);

  if (tsMs >= todayStart) {
    const diffMs = Math.max(0, nowMs - tsMs);
    if (diffMs < 60_000) return "Just now";
    if (diffMs < 3_600_000) return `${Math.floor(diffMs / 60_000)}m ago`;
    return `${Math.floor(diffMs / 3_600_000)}h ago`;
  }

  const date = new Date(tsMs);
  if (tsMs >= yesterdayStart) {
    return `Yesterday · ${formatHm(date)}`;
  }

  const month = SHORT_MONTHS[date.getMonth()];
  return `${month} ${date.getDate()} · ${formatHm(date)}`;
}

export function formatFullNotificationTime(postedUnix: number): string {
  if (!postedUnix || postedUnix <= 0) return "";
  return new Date(postedUnix * 1000).toLocaleString();
}

export interface NotificationFilterOptions {
  selectedPackage?: string | null;
  query?: string;
  maskedPackages?: ReadonlySet<string>;
}

export function filterNotifications(
  items: PhoneHubNotification[],
  options: NotificationFilterOptions,
): PhoneHubNotification[] {
  const pkg = options.selectedPackage && options.selectedPackage !== "all"
    ? options.selectedPackage
    : null;
  const q = (options.query ?? "").trim().toLowerCase();

  return items.filter((item) => {
    if (pkg && item.package !== pkg) return false;
    if (!q) return true;
    if (item.appLabel.toLowerCase().includes(q) || item.package.toLowerCase().includes(q)) {
      return true;
    }
    if (options.maskedPackages?.has(item.package)) {
      return false;
    }
    return item.title.toLowerCase().includes(q) || item.text.toLowerCase().includes(q);
  });
}

export function getNotificationHideId(
  n: Pick<PhoneHubNotification, "package" | "key" | "postedUnix">,
): string {
  return `${n.package}::${n.key}::${n.postedUnix}`;
}

export interface AppAvatarColorPair {
  bg: string;
  fg: string;
}

/**
 * 10 dark-theme-friendly background/foreground pairs for deterministic app letter avatars.
 * Every pair is verified >= 4.5:1 WCAG AA contrast in notificationTidy.test.ts.
 */
export const APP_AVATAR_PALETTE: readonly AppAvatarColorPair[] = [
  { bg: "#2563EB", fg: "#FFFFFF" },
  { bg: "#B45309", fg: "#FFFFFF" },
  { bg: "#047857", fg: "#FFFFFF" },
  { bg: "#BE123C", fg: "#FFFFFF" },
  { bg: "#6D28D9", fg: "#FFFFFF" },
  { bg: "#0E7490", fg: "#FFFFFF" },
  { bg: "#C2410C", fg: "#FFFFFF" },
  { bg: "#0F766E", fg: "#FFFFFF" },
  { bg: "#A21CAF", fg: "#FFFFFF" },
  { bg: "#4D7C0F", fg: "#FFFFFF" },
];

export function hashPackageName(packageName: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < packageName.length; i += 1) {
    hash ^= packageName.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

export function getAppAvatarColors(packageName: string): AppAvatarColorPair {
  const index = hashPackageName(packageName) % APP_AVATAR_PALETTE.length;
  return APP_AVATAR_PALETTE[index];
}

function leadingUnicodeLetter(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const first = Array.from(trimmed)[0];
  if (first && /\p{L}/u.test(first)) {
    return first.toUpperCase();
  }
  return null;
}

/**
 * Picks the uppercase avatar letter from the resolved app label; if the label is
 * unresolved or begins with a digit, emoji, or symbol, falls back to the first
 * letter of the package's last segment.
 */
export function getAppAvatarLetter(appLabel: string, packageName: string): string {
  const trimmedLabel = appLabel.trim();
  const trimmedPkg = packageName.trim();
  if (trimmedLabel && trimmedLabel !== trimmedPkg) {
    const labelLetter = leadingUnicodeLetter(trimmedLabel);
    if (labelLetter) return labelLetter;
  }

  const lastSegment = trimmedPkg.split(".").filter(Boolean).pop() ?? "";
  const segmentLeading = leadingUnicodeLetter(lastSegment);
  if (segmentLeading) return segmentLeading;

  const segmentAnyLetter = Array.from(lastSegment).find((ch) => /\p{L}/u.test(ch));
  if (segmentAnyLetter) return segmentAnyLetter.toUpperCase();

  return "?";
}

export interface AppAvatarStyle {
  letter: string;
  bg: string;
  fg: string;
}

export function getAppAvatar(appLabel: string, packageName: string): AppAvatarStyle {
  const { bg, fg } = getAppAvatarColors(packageName);
  return {
    letter: getAppAvatarLetter(appLabel, packageName),
    bg,
    fg,
  };
}
