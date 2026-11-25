import { describe, expect, it } from "vitest";
import type { PhoneHubNotification } from "@/types/phone";
import {
  APP_AVATAR_PALETTE,
  bucketByLocalDay,
  collapseNotificationWhitespace,
  filterNotifications,
  formatRelativeNotificationTime,
  getAppAvatar,
  getAppAvatarColors,
  getAppAvatarLetter,
  getNotificationHideId,
  shouldShowNotificationExpand,
  stackSameTitleNotifications,
} from "./notificationTidy";

function makeNotif(overrides: Partial<PhoneHubNotification> = {}): PhoneHubNotification {
  return {
    key: "k1",
    package: "com.example.chat",
    appLabel: "Example Chat",
    title: "Alex",
    text: "Hello world",
    postedUnix: 1_700_000_000,
    ...overrides,
  };
}

describe("notificationTidy (SPEC-33 Phase 1)", () => {
  describe("collapseNotificationWhitespace", () => {
    it("collapses CRLF, tabs, redundant spaces, and blank lines into compact lines", () => {
      const raw = "  Line one\t\twith   spaces \r\n\r\n\r\n  Line   two \n  \n\tLine three  \r\n";
      expect(collapseNotificationWhitespace(raw)).toBe(
        "Line one with spaces\nLine two\nLine three",
      );
    });

    it("returns empty string for whitespace-only input", () => {
      expect(collapseNotificationWhitespace(" \r\n \t \n ")).toBe("");
    });
  });

  describe("shouldShowNotificationExpand", () => {
    it("returns false for short single-line or two-line text without collapsed blank lines", () => {
      const raw = "Payment received\nReady to view";
      const collapsed = collapseNotificationWhitespace(raw);
      expect(shouldShowNotificationExpand(raw, collapsed)).toBe(false);
    });

    it("returns true when collapsed text has more than 2 lines", () => {
      const raw = "Line 1\nLine 2\nLine 3";
      const collapsed = collapseNotificationWhitespace(raw);
      expect(shouldShowNotificationExpand(raw, collapsed)).toBe(true);
    });

    it("returns true when raw text had blank lines collapsed", () => {
      const raw = "Line 1\n\nLine 2";
      const collapsed = collapseNotificationWhitespace(raw);
      expect(shouldShowNotificationExpand(raw, collapsed)).toBe(true);
    });

    it("returns true when single-line text exceeds clamp length threshold", () => {
      const raw = "A".repeat(130);
      const collapsed = collapseNotificationWhitespace(raw);
      expect(shouldShowNotificationExpand(raw, collapsed)).toBe(true);
    });
  });

  describe("stackSameTitleNotifications", () => {
    it("clusters notifications sharing (package, normalizedTitle) with newest on top", () => {
      const items: PhoneHubNotification[] = [
        makeNotif({ key: "k1", title: "  Build Alert ", text: "Oldest", postedUnix: 100 }),
        makeNotif({ key: "k2", title: "build alert", text: "Middle", postedUnix: 200 }),
        makeNotif({ key: "k3", title: "BUILD   ALERT", text: "Newest", postedUnix: 300 }),
        makeNotif({ key: "k4", title: "Different Title", text: "Solo", postedUnix: 250 }),
        makeNotif({
          key: "k5",
          package: "com.example.mail",
          appLabel: "Example Mail",
          title: "Build Alert",
          text: "Other app",
          postedUnix: 280,
        }),
      ];

      const stacks = stackSameTitleNotifications(items);
      expect(stacks).toHaveLength(3);

      expect(stacks[0].latest.key).toBe("k3");
      expect(stacks[0].olderItems.map((n) => n.key)).toEqual(["k2", "k1"]);

      expect(stacks[1].latest.key).toBe("k5");
      expect(stacks[1].olderItems).toEqual([]);

      expect(stacks[2].latest.key).toBe("k4");
      expect(stacks[2].olderItems).toEqual([]);
    });
  });

  describe("bucketByLocalDay", () => {
    it("partitions items into Today, Yesterday, and Earlier based on local midnight", () => {
      const now = new Date(2026, 10, 25, 14, 30, 0).getTime();
      const todayItem = makeNotif({
        key: "today",
        postedUnix: Math.floor(new Date(2026, 10, 25, 0, 5, 0).getTime() / 1000),
      });
      const yesterdayItem = makeNotif({
        key: "yesterday",
        postedUnix: Math.floor(new Date(2026, 10, 24, 23, 55, 0).getTime() / 1000),
      });
      const earlierItem = makeNotif({
        key: "earlier",
        postedUnix: Math.floor(new Date(2026, 10, 20, 12, 0, 0).getTime() / 1000),
      });

      const buckets = bucketByLocalDay([earlierItem, todayItem, yesterdayItem], now);
      expect(buckets.map((b) => b.label)).toEqual(["Today", "Yesterday", "Earlier"]);
      expect(buckets[0].items.map((n) => n.key)).toEqual(["today"]);
      expect(buckets[1].items.map((n) => n.key)).toEqual(["yesterday"]);
      expect(buckets[2].items.map((n) => n.key)).toEqual(["earlier"]);
    });

    it("skips empty sections", () => {
      const now = new Date(2026, 10, 25, 14, 30, 0).getTime();
      const todayItem = makeNotif({
        key: "today",
        postedUnix: Math.floor(new Date(2026, 10, 25, 14, 0, 0).getTime() / 1000),
      });

      const buckets = bucketByLocalDay([todayItem], now);
      expect(buckets.map((b) => b.label)).toEqual(["Today"]);
    });
  });

  describe("formatRelativeNotificationTime", () => {
    const now = new Date(2026, 10, 25, 14, 30, 0).getTime();

    it("formats Just now, minutes ago, hours ago, Yesterday, and older dates", () => {
      const sec = (d: Date) => Math.floor(d.getTime() / 1000);
      expect(formatRelativeNotificationTime(sec(new Date(2026, 10, 25, 14, 29, 30)), now)).toBe(
        "Just now",
      );
      expect(formatRelativeNotificationTime(sec(new Date(2026, 10, 25, 14, 25, 0)), now)).toBe(
        "5m ago",
      );
      expect(formatRelativeNotificationTime(sec(new Date(2026, 10, 25, 12, 10, 0)), now)).toBe(
        "2h ago",
      );
      expect(formatRelativeNotificationTime(sec(new Date(2026, 10, 24, 18, 40, 0)), now)).toBe(
        "Yesterday · 18:40",
      );
      expect(formatRelativeNotificationTime(sec(new Date(2026, 10, 21, 9, 15, 0)), now)).toBe(
        "Nov 21 · 09:15",
      );
      expect(formatRelativeNotificationTime(0, now)).toBe("");
    });
  });

  describe("filterNotifications", () => {
    const items: PhoneHubNotification[] = [
      makeNotif({
        key: "k1",
        package: "com.example.chat",
        appLabel: "Example Chat",
        title: "Sam",
        text: "Lunch at noon?",
      }),
      makeNotif({
        key: "k2",
        package: "com.example.bank",
        appLabel: "Example Bank",
        title: "Transfer",
        text: "Receipt 45000",
      }),
    ];

    it("filters by package chip and case-insensitive search across label, package, title, and text", () => {
      expect(
        filterNotifications(items, { selectedPackage: "com.example.bank", query: "" }).map(
          (n) => n.key,
        ),
      ).toEqual(["k2"]);

      expect(
        filterNotifications(items, { selectedPackage: null, query: "NOON" }).map((n) => n.key),
      ).toEqual(["k1"]);

      expect(
        filterNotifications(items, { selectedPackage: null, query: "example.bank" }).map(
          (n) => n.key,
        ),
      ).toEqual(["k2"]);
    });

    it("does not match masked title/text for sensitive packages while still matching appLabel/package", () => {
      const masked = new Set(["com.example.bank"]);
      expect(
        filterNotifications(items, { query: "45000", maskedPackages: masked }).map((n) => n.key),
      ).toEqual([]);
      expect(
        filterNotifications(items, { query: "example bank", maskedPackages: masked }).map(
          (n) => n.key,
        ),
      ).toEqual(["k2"]);
    });
  });

  describe("getNotificationHideId", () => {
    it("uses composite (package, key, postedUnix)", () => {
      const id = getNotificationHideId(
        makeNotif({ package: "com.example.a", key: "k1", postedUnix: 42 }),
      );
      expect(id).toBe("com.example.a::k1::42");
    });
  });

  describe("app letter avatar helpers", () => {
    function relativeLuminance(hex: string): number {
      const clean = hex.replace("#", "");
      const r = Number.parseInt(clean.slice(0, 2), 16) / 255;
      const g = Number.parseInt(clean.slice(2, 4), 16) / 255;
      const b = Number.parseInt(clean.slice(4, 6), 16) / 255;
      const toLinear = (c: number) =>
        c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
      return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
    }

    function contrastRatio(hexA: string, hexB: string): number {
      const lumA = relativeLuminance(hexA);
      const lumB = relativeLuminance(hexB);
      const lighter = Math.max(lumA, lumB);
      const darker = Math.min(lumA, lumB);
      return (lighter + 0.05) / (darker + 0.05);
    }

    it("returns deterministic colors across repeated calls for the same package", () => {
      const first = getAppAvatarColors("com.example.bank");
      const second = getAppAvatarColors("com.example.bank");
      expect(first).toEqual(second);
      expect(APP_AVATAR_PALETTE).toContainEqual(first);
    });

    it("spreads different packages across the palette", () => {
      const packages = [
        "com.example.bank",
        "com.example.chat",
        "com.example.mail",
        "com.example.wallet",
        "com.example.shop",
        "com.example.music",
        "com.example.maps",
        "com.example.notes",
        "com.example.news",
        "com.example.cloud",
        "org.example.alpha",
        "org.example.beta",
        "io.example.gamma",
        "net.example.delta",
        "app.example.epsilon",
        "dev.example.zeta",
      ];
      const usedColors = new Set(packages.map((p) => getAppAvatarColors(p).bg));
      expect(usedColors.size).toBeGreaterThanOrEqual(6);
    });

    it("meets >= 4.5:1 WCAG contrast ratio for every palette pair", () => {
      expect(APP_AVATAR_PALETTE.length).toBeGreaterThanOrEqual(8);
      expect(APP_AVATAR_PALETTE.length).toBeLessThanOrEqual(10);
      for (const pair of APP_AVATAR_PALETTE) {
        expect(contrastRatio(pair.fg, pair.bg)).toBeGreaterThanOrEqual(4.5);
      }
    });

    it("extracts uppercase first letter from resolved label and falls back to package last segment for digits/emoji/unresolved", () => {
      expect(getAppAvatarLetter("chat", "com.example.chat")).toBe("C");
      expect(getAppAvatarLetter("  Bank ", "com.example.bank")).toBe("B");
      // Digit-prefixed label falls back to last segment of package
      expect(getAppAvatarLetter("1Password", "com.example.onepass")).toBe("O");
      expect(getAppAvatarLetter("7-Eleven", "com.example.store")).toBe("S");
      // Emoji-prefixed label falls back to last segment of package
      expect(getAppAvatarLetter("💬 Chat", "com.example.messenger")).toBe("M");
      // Unresolved label equal to package name falls back to last segment
      expect(getAppAvatarLetter("com.example.wallet", "com.example.wallet")).toBe("W");
      expect(getAppAvatarLetter("", "")).toBe("?");

      const combined = getAppAvatar("1Password", "com.example.onepass");
      expect(combined.letter).toBe("O");
      expect(combined.bg).toMatch(/^#[0-9A-F]{6}$/i);
      expect(combined.fg).toMatch(/^#[0-9A-F]{6}$/i);
    });
  });
});
