import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MorePopover } from "./MorePopover";
import type { ControlItem } from "@/lib/controlBarModel";

const items: ControlItem[] = [
  { id: "notifications", label: "Notifications", kind: "action" },
  { id: "wifi", label: "Wi-Fi", kind: "toggle", pressed: true },
  { id: "dnd", label: "Do Not Disturb", kind: "toggle", pressed: false },
];

describe("MorePopover keyboard behavior", () => {
  let host: HTMLDivElement;
  let root: Root;
  const onActivate = vi.fn();

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    act(() => root.render(<MorePopover items={items} icons={{}} onActivate={onActivate} />));
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    onActivate.mockReset();
  });

  const more = () => host.querySelector<HTMLButtonElement>('button[aria-label="More"]')!;
  const panelButtons = () => Array.from(document.querySelectorAll<HTMLButtonElement>('[aria-label="More controls"] button'));
  const key = (k: string) =>
    act(() => {
      (document.activeElement as HTMLElement).dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
    });

  it("opens on click, focuses the first item and exposes aria-pressed", () => {
    expect(more().getAttribute("aria-expanded")).toBe("false");
    act(() => more().click());
    expect(more().getAttribute("aria-expanded")).toBe("true");
    const btns = panelButtons();
    expect(btns).toHaveLength(3);
    expect(document.activeElement).toBe(btns[0]);
    expect(btns[1].getAttribute("aria-pressed")).toBe("true");
    expect(btns[0].hasAttribute("aria-pressed")).toBe(false);
  });

  it("moves with arrow keys and wraps", () => {
    act(() => more().click());
    const btns = panelButtons();
    key("ArrowDown");
    expect(document.activeElement).toBe(btns[1]);
    key("ArrowUp");
    key("ArrowUp");
    expect(document.activeElement).toBe(btns[2]);
  });

  it("Escape closes and returns focus to the More button", () => {
    act(() => more().click());
    key("Escape");
    expect(panelButtons()).toHaveLength(0);
    expect(more().getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(more());
  });

  it("activates an item with click", () => {
    act(() => more().click());
    act(() => panelButtons()[1].click());
    expect(onActivate).toHaveBeenCalledWith(items[1]);
  });

  it("shows a tooltip on keyboard focus of the More button", () => {
    act(() => more().focus());
    expect(document.querySelector('[role="tooltip"]')?.textContent).toBe("More");
    act(() => more().blur());
    expect(document.querySelector('[role="tooltip"]')).toBeNull();
  });
});
