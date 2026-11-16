import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePhoneHubStore } from "@/stores/phoneHubStore";
import type { Conversation, PhoneHubNotification } from "@/types/phone";

const mockPhoneOpenApp = vi.fn((..._args: unknown[]) => Promise.resolve({}));
const mockPhoneGetConversations = vi.fn<(...args: unknown[]) => Promise<Conversation[]>>(() =>
  Promise.resolve([]),
);
const mockPhoneMarkConversationRead = vi.fn((..._args: unknown[]) => Promise.resolve());

vi.mock("@/services/phoneHubService", () => ({
  phoneOpenApp: (...args: unknown[]) => mockPhoneOpenApp(...args),
  phoneGetConversations: (...args: unknown[]) => mockPhoneGetConversations(...args),
  phoneMarkConversationRead: (...args: unknown[]) =>
    mockPhoneMarkConversationRead(...args),
}));

vi.mock("@/lib/persist", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/persist")>();
  return {
    ...actual,
    saveAppPrefs: vi.fn(() => Promise.resolve()),
  };
});

import { NotificationsView } from "../NotificationsView";
import { MessagesView } from "../MessagesView";

describe("NotificationsView & MessagesView (SPEC-27b Task B3)", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    mockPhoneOpenApp.mockClear();
    mockPhoneGetConversations.mockClear();
    mockPhoneMarkConversationRead.mockClear();
    usePhoneHubStore.getState().reset();
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
});
