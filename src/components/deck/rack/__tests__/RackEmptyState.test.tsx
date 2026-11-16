import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { RackEmptyState } from "../RackEmptyState";

describe("RackEmptyState", () => {
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

  it("renders empty state instructions", () => {
    act(() => {
      root.render(<RackEmptyState />);
    });
    expect(container.textContent).toContain("No devices detected");
    expect(container.textContent).toContain("Pair Device");
  });
});
