import {
  REGISTRY,
  SECTIONS,
  type SectionId,
  type SettingEntry,
} from "@/components/settings/registry";

export interface FilterResult {
  query: string;
  active: boolean;
  /** Ids of matching entries (every id when the query is empty). */
  matches: Set<string>;
  /** Matching entries per section. */
  counts: Record<SectionId, number>;
  total: number;
  /** Sections that have at least one match, in rail order. */
  sections: SectionId[];
  /** True when an Advanced row matches an active query (the disclosure auto-expands). */
  advancedOpen: boolean;
}

export function entryMatches(e: SettingEntry, needle: string): boolean {
  if (!needle) return true;
  const hay = [e.label, e.helper ?? "", ...(e.keywords ?? [])].join("\n").toLowerCase();
  return hay.includes(needle);
}

/** Case-insensitive substring match over label, helper and keywords (SPEC-28 §7). */
export function filterSettings(query: string, entries: SettingEntry[] = REGISTRY): FilterResult {
  const needle = query.trim().toLowerCase();
  const matches = new Set<string>();
  const counts = Object.fromEntries(SECTIONS.map((s) => [s.id, 0])) as Record<SectionId, number>;
  for (const e of entries) {
    if (entryMatches(e, needle)) {
      matches.add(e.id);
      counts[e.section] += 1;
    }
  }
  return {
    query: query.trim(),
    active: needle.length > 0,
    matches,
    counts,
    total: matches.size,
    sections: SECTIONS.filter((s) => counts[s.id] > 0).map((s) => s.id),
    advancedOpen: needle.length > 0 && counts.advanced > 0,
  };
}
