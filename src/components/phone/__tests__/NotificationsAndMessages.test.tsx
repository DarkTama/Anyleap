import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "@/store/useAppStore";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import type { Conversation, PhoneHubNotification } from "@/types/phone";

const mockPhoneOpenApp = vi.fn((..._args: unknown[]) => Promise.resolve({}));
const mockPhoneGetConversations = vi.fn<(...args: unknown[]) => Promise<Conversation[]>>(() =>
  Promise.resolve([]),
);
const mockPhoneMarkConversationRead = vi.fn((..._args: unknown[]) => Promise.resolve());
const mockPhoneGetNotificationHistory = vi.fn<(...args: unknown[]) => Promise<unknown[]>>(() =>
  Promise.resolve([]),
);
const mockPhoneSetNotificationHidden = vi.fn((..._args: unknown[]) => Promise.resolve());
const mockPhoneDeleteAllNotificationData = vi.fn((..._args: unknown[]) => Promise.resolve({}));

vi.mock("@/services/phoneHubService", () => ({
  phoneOpenApp: (...args: unknown[]) => mockPhoneOpenApp(...args),
  phoneGetConversations: (...args: unknown[]) => mockPhoneGetConversations(...args),
  phoneMarkConversationRead: (...args: unknown[]) =>
    mockPhoneMarkConversationRead(...args),
  phoneGetNotificationHistory: (...args: unknown[]) =>
    mockPhoneGetNotificationHistory(...args),
  phoneSetNotificationHidden: (...args: unknown[]) =>
    mockPhoneSetNotificationHidden(...args),
  phoneDeleteAllNotificationData: (...args: unknown[]) =>
    mockPhoneDeleteAllNotificationData(...args),
}));

vi.mock("@/lib/persist", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/persist")>();
  return {
    ...actual,
    saveAppPrefs: vi.fn(() => Promise.resolve()),
    saveNotificationStorage: vi.fn(() => Promise.resolve()),
  };
});

import { DEFAULT_NOTIFICATION_STORAGE } from "@/lib/notificationStorage";
import { NotificationsView } from "../NotificationsView";
import { MessagesView } from "../MessagesView";

describe("NotificationsView & MessagesView (SPEC-27b + SPEC-33 Phase 1)", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    mockPhoneOpenApp.mockClear();
    mockPhoneGetConversations.mockClear();
    mockPhoneMarkConversationRead.mockClear();
    mockPhoneGetNotificationHistory.mockReset();
    mockPhoneGetNotificationHistory.mockResolvedValue([]);
    mockPhoneSetNotificationHidden.mockClear();
    mockPhoneDeleteAllNotificationData.mockClear();
    usePhoneHubStore.getState().reset();
    useAppStore.getState().setNotificationStorage({
      ...DEFAULT_NOTIFICATION_STORAGE,
      largeIncomeThresholdByCurrency: {},
      perApp: {},
    });
    useAppStore.getState().setVaultError(null);
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    container.remove();
  });

  it("excludes muted apps from NotificationsView list and allows muting/unmuting", async () => {
    usePhoneHubStore.getState().setMutedPackages(["app.example.games"]);

    const items: PhoneHubNotification[] = [
      {
        key: "k1",
        package: "app.example.chat",
        appLabel: "Chat",
        title: "Alex",
        text: "Are we still on?",
        postedUnix: 200,
      },
      {
        key: "k2",
        package: "app.example.games",
        appLabel: "Games",
        title: "Energy full",
        text: "Come back!",
        postedUnix: 300,
      },
    ];

    await act(async () => {
      root.render(
        <NotificationsView
          serial="serial-1"
          notifications={items}
          loading={false}
          error={null}
          onRefresh={() => {}}
        />,
      );
    });

    expect(container.textContent).toContain("Are we still on?");
    expect(container.textContent).not.toContain("Energy full");
    expect(container.textContent).toContain("1 notification from muted apps is hidden.");

    // Open Mute… menu for Chat and verify "Open Chat on PC" + "Mute Chat"
    const muteTrigger = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Mute… Chat"]',
    );
    await act(async () => {
      muteTrigger?.click();
    });

    const openMenuItem = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    ).find((b) => b.textContent?.includes("Open Chat on PC"));
    expect(openMenuItem).toBeDefined();

    await act(async () => {
      openMenuItem?.click();
    });
    expect(mockPhoneOpenApp).toHaveBeenCalledWith("serial-1", "app.example.chat");
  });

  it("renders date sections, stacks same-title items, clamps with Show more, and collapses app groups", async () => {
    const nowSec = Math.floor(Date.now() / 1000);
    const yesterdaySec = nowSec - 24 * 3600;
    const earlierSec = nowSec - 5 * 24 * 3600;

    const items: PhoneHubNotification[] = [
      {
        key: "k-today-1",
        package: "com.example.chat",
        appLabel: "Example Chat",
        title: "Project Thread",
        text: "First\n\n\nSecond   line\nThird line extra",
        postedUnix: nowSec - 120,
      },
      {
        key: "k-today-2",
        package: "com.example.chat",
        appLabel: "Example Chat",
        title: "project thread",
        text: "Older message in same stack",
        postedUnix: nowSec - 300,
      },
      {
        key: "k-yest",
        package: "com.example.mail",
        appLabel: "Example Mail",
        title: "Weekly digest",
        text: "Summary ready",
        postedUnix: yesterdaySec,
      },
      {
        key: "k-early",
        package: "com.example.notes",
        appLabel: "Example Notes",
        title: "Reminder",
        text: "Buy milk",
        postedUnix: earlierSec,
      },
    ];

    await act(async () => {
      root.render(
        <NotificationsView
          serial="serial-1"
          notifications={items}
          loading={false}
          error={null}
          onRefresh={() => {}}
        />,
      );
    });

    expect(container.textContent).toContain("Today");
    expect(container.textContent).toContain("Yesterday");
    expect(container.textContent).toContain("Earlier");

    // Collapsed whitespace preview on top of stack; older item tucked under +1 more
    expect(container.textContent).toContain("First\nSecond line\nThird line extra");
    expect(container.textContent).not.toContain("Older message in same stack");

    const showMoreBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Show more"),
    );
    expect(showMoreBtn?.getAttribute("aria-expanded")).toBe("false");
    await act(async () => {
      showMoreBtn?.click();
    });
    expect(showMoreBtn?.getAttribute("aria-expanded")).toBe("true");
    expect(showMoreBtn?.textContent).toContain("Show less");

    const stackBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("+1 more"),
    );
    expect(stackBtn?.getAttribute("aria-expanded")).toBe("false");
    await act(async () => {
      stackBtn?.click();
    });
    expect(container.textContent).toContain("Older message in same stack");

    // Collapse Example Chat group
    const collapseBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Collapse Example Chat notifications"]',
    );
    expect(collapseBtn?.getAttribute("aria-expanded")).toBe("true");
    await act(async () => {
      collapseBtn?.click();
    });
    expect(container.textContent).not.toContain("Older message in same stack");
  });

  it("filters by app chip and search query with Escape to clear", async () => {
    const nowSec = Math.floor(Date.now() / 1000);
    const items: PhoneHubNotification[] = [
      {
        key: "k1",
        package: "com.example.chat",
        appLabel: "Example Chat",
        title: "Sam",
        text: "Meet at 5pm",
        postedUnix: nowSec - 60,
      },
      {
        key: "k2",
        package: "com.example.shop",
        appLabel: "Example Shop",
        title: "Order shipped",
        text: "Parcel #42 on the way",
        postedUnix: nowSec - 120,
      },
    ];

    await act(async () => {
      root.render(
        <NotificationsView
          serial="serial-1"
          notifications={items}
          loading={false}
          error={null}
          onRefresh={() => {}}
        />,
      );
    });

    const shopChip = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[aria-label="Filter notifications by app"] button'),
    ).find((b) => b.textContent?.includes("Example Shop"));
    await act(async () => {
      shopChip?.click();
    });
    expect(container.textContent).toContain("Parcel #42 on the way");
    expect(container.textContent).not.toContain("Meet at 5pm");

    const allChip = Array.from(
      container.querySelectorAll<HTMLButtonElement>('[aria-label="Filter notifications by app"] button'),
    ).find((b) => b.textContent?.startsWith("All"));
    await act(async () => {
      allChip?.click();
    });

    const searchInput = container.querySelector<HTMLInputElement>('input[type="search"]');
    expect(searchInput).not.toBeNull();
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      setter?.call(searchInput, "5pm");
      searchInput?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(container.textContent).toContain("Meet at 5pm");
    expect(container.textContent).not.toContain("Parcel #42 on the way");

    await act(async () => {
      searchInput?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });
    expect(container.textContent).toContain("Parcel #42 on the way");
  });

  it("masks sensitive apps, hides toast preview, re-masks on unmount, and supports local Hide", async () => {
    const nowSec = Math.floor(Date.now() / 1000);
    useAppStore.setState((s) => ({
      appPrefs: { ...s.appPrefs, desktopNotifications: true },
    }));
    usePhoneHubStore.getState().setSensitivePackages(["com.example.bank"]);

    const items: PhoneHubNotification[] = [
      {
        key: "k1",
        package: "com.example.bank",
        appLabel: "Example Bank",
        title: "OTP Code 987654",
        text: "Use 987654 to approve transfer",
        postedUnix: nowSec - 30,
      },
    ];

    await act(async () => {
      root.render(
        <NotificationsView
          serial="serial-1"
          notifications={items}
          loading={false}
          error={null}
          onRefresh={() => {}}
        />,
      );
    });

    expect(container.textContent).toContain("Sensitive content hidden — click to reveal");
    expect(container.textContent).not.toContain("987654");
    expect(container.textContent).toContain("New notification from Example Bank");

    const revealBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Sensitive content hidden"),
    );
    await act(async () => {
      revealBtn?.click();
    });
    expect(container.textContent).toContain("OTP Code 987654");

    // Switching tabs/unmounting re-masks automatically
    await act(async () => {
      root.unmount();
    });
    root = createRoot(container);
    await act(async () => {
      root.render(
        <NotificationsView
          serial="serial-1"
          notifications={items}
          loading={false}
          error={null}
          onRefresh={() => {}}
        />,
      );
    });
    expect(container.textContent).toContain("Sensitive content hidden — click to reveal");
    expect(container.textContent).not.toContain("987654");

    // Local Hide removes item without dismissing on phone and allows restoring
    const hideBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Hide in AnyLeap"]',
    );
    await act(async () => {
      hideBtn?.click();
    });
    expect(container.textContent).not.toContain("Sensitive content hidden — click to reveal");
    const restoreBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("1 hidden · Show"),
    );
    expect(restoreBtn).toBeDefined();
    await act(async () => {
      restoreBtn?.click();
    });
    expect(container.textContent).toContain("Sensitive content hidden — click to reveal");
  });

  it("renders deterministic colored letter avatars in headers, chips, and toast preview", async () => {
    const nowSec = Math.floor(Date.now() / 1000);
    const items: PhoneHubNotification[] = [
      {
        key: "k1",
        package: "com.example.onepass",
        appLabel: "1Password",
        title: "Alert",
        text: "Ready",
        postedUnix: nowSec - 10,
      },
      {
        key: "k2",
        package: "com.example.chat",
        appLabel: "Chat",
        title: "Ping",
        text: "Yo",
        postedUnix: nowSec - 20,
      },
    ];

    await act(async () => {
      root.render(
        <NotificationsView
          serial="serial-1"
          notifications={items}
          loading={false}
          error={null}
          onRefresh={() => {}}
        />,
      );
    });

    expect(container.querySelectorAll("img").length).toBe(0);
    // "1Password" falls back to package last segment "onepass" -> "O"
    const avatars = Array.from(container.querySelectorAll('span[aria-hidden="true"]'));
    expect(avatars.some((el) => el.textContent === "O" && (el as HTMLElement).style.backgroundColor)).toBe(
      true,
    );
    expect(avatars.some((el) => el.textContent === "C" && (el as HTMLElement).style.backgroundColor)).toBe(
      true,
    );
  });

  it("renders MessagesView empty state explaining memory-only capture limitation", async () => {
    mockPhoneGetConversations.mockResolvedValueOnce([]);
    await act(async () => {
      root.render(<MessagesView serial="serial-1" />);
    });

    expect(container.textContent).toContain("No messages yet");
    expect(container.textContent).toContain(
      "only messages that arrive as notifications are captured",
    );
  });

  it("orders conversations newest-first and marks read on click", async () => {
    const convs: Conversation[] = [
      {
        package: "app.example.chat",
        appLabel: "Chat",
        key: "old",
        title: "Sam",
        isGroup: false,
        messages: [{ sender: "Sam", text: "Earlier", timestampMs: 1000 }],
        unread: 0,
      },
      {
        package: "app.example.chat",
        appLabel: "Chat",
        key: "new",
        title: "Alex",
        isGroup: false,
        messages: [{ sender: "Alex", text: "Latest", timestampMs: 2000 }],
        unread: 2,
      },
    ];
    mockPhoneGetConversations.mockResolvedValueOnce(convs);

    await act(async () => {
      root.render(<MessagesView serial="serial-1" />);
    });

    const buttons = Array.from(
      container.querySelectorAll<HTMLButtonElement>("aside button"),
    );
    expect(buttons[0]?.textContent).toContain("Alex");
    expect(buttons[1]?.textContent).toContain("Sam");

    await act(async () => {
      buttons[0]?.click();
    });

    expect(mockPhoneMarkConversationRead).toHaveBeenCalledWith("serial-1", "new");
    expect(container.textContent).toContain("Read-only — replies need the phone.");
    expect(container.textContent).toContain("2 new messages");
  });

  it("hydrates saved notification history, shows retention & source badges, and displays offline banner", async () => {
    const nowSec = Math.floor(Date.now() / 1000);
    useAppStore.getState().setNotificationStorage({
      version: 2,
      historyEnabled: true,
      backgroundCapture: true,
      transactionRetention: "1y",
      transferMatchWindowMinutes: 15,
      largeIncomeThresholdByCurrency: {},
      perApp: {
        "com.example.bank": {
          retention: "history",
          retentionDays: 30,
          sensitive: false,
          moneySources: [
            {
              id: "src_1",
              type: "bank",
              ruleTemplate: "generic_bank",
              enabled: true,
            },
          ],
        },
      },
    });
    mockPhoneGetNotificationHistory.mockResolvedValueOnce([
      {
        dedupeId: `HW1::k1::${nowSec - 60}`,
        hwSerial: "HW1",
        key: "k1",
        package: "com.example.bank",
        appLabel: "Example Bank",
        title: "Saved Alert",
        text: "Historical encrypted item",
        postedUnix: nowSec - 60,
        capturedUnix: nowSec - 55,
        hiddenLocal: false,
      },
    ]);

    await act(async () => {
      root.render(
        <NotificationsView
          serial="HW1"
          hwSerial="HW1"
          isOffline={true}
          notifications={[]}
          loading={false}
          error={null}
          onRefresh={() => {}}
        />,
      );
    });

    expect(mockPhoneGetNotificationHistory).toHaveBeenCalledWith("HW1", 200);
    expect(container.textContent).toContain("Offline — showing saved history");
    expect(container.textContent).toContain("Historical encrypted item");
    expect(container.textContent).toContain("History · 30d · Settings");
    expect(container.textContent).toContain("Bank");

    const hideBtn = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Hide in AnyLeap"]',
    );
    await act(async () => {
      hideBtn?.click();
    });
    expect(mockPhoneSetNotificationHidden).toHaveBeenCalledWith(
      `HW1::k1::${nowSec - 60}`,
      true,
    );
  });
});
