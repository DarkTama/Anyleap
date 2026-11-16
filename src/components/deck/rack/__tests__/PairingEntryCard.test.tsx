import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PairingEntryCard } from "../PairingEntryCard";

describe("PairingEntryCard", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
  });

  it("renders pairing callout and invokes onOpenPair", () => {
    const onOpenPair = vi.fn();
    act(() => {
      root.render(<PairingEntryCard onOpenPair={onOpenPair} />);
    });
    expect(container.textContent).toContain("Add a phone");
    const btn = container.querySelector("button");
    expect(btn?.textContent).toContain("Pair Device");
    act(() => {
      btn?.click();
    });
    expect(onOpenPair).toHaveBeenCalledTimes(1);
  });
});
