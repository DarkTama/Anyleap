import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { RackHeader } from "../RackHeader";

describe("RackHeader", () => {
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

  it("renders title, count, and transport label", () => {
    act(() => {
      root.render(<RackHeader deviceCount={3} />);
    });
    expect(container.textContent).toContain("Device Rack");
    expect(container.textContent).toContain("3");
    expect(container.textContent).toContain("USB / WI-FI");
  });
});
