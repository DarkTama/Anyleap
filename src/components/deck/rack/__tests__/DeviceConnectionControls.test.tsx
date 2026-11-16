import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DeviceConnectionControls } from "../DeviceConnectionControls";

describe("DeviceConnectionControls", () => {
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

  it("renders Launch button for connected device and fires onLaunch", () => {
    const onLaunch = vi.fn();
    act(() => {
      root.render(
        <DeviceConnectionControls
          serial="192.168.1.24:5555"
          status="connected"
          hasSession={false}
          isBusy={false}
          showDoctor={false}
          showDisconnect={true}
          canQuickAction={true}
          canForget={false}
          quickAction={null}
          quickInput=""
          quickNote={null}
          onStop={vi.fn()}
          onLaunch={onLaunch}
          onDoctor={vi.fn()}
          onDisconnect={vi.fn()}
          onOpenQuickInput={vi.fn()}
          onQuickInputChange={vi.fn()}
          onSubmitQuickInput={vi.fn()}
          onCloseQuickInput={vi.fn()}
        />,
      );
    });

    const launchBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Launch"),
    );
    expect(launchBtn).toBeDefined();
    act(() => {
      launchBtn?.click();
    });
    expect(onLaunch).toHaveBeenCalledTimes(1);
  });

  it("renders Stop button when mirroring and quick input popover when active", () => {
    act(() => {
      root.render(
        <DeviceConnectionControls
          serial="abc123"
          status="mirroring"
          hasSession={true}
          isBusy={false}
          showDoctor={false}
          showDisconnect={false}
          canQuickAction={true}
          canForget={false}
          quickAction={{ serial: "abc123", kind: "url" }}
          quickInput="https://example.com"
          quickNote={null}
          onStop={vi.fn()}
          onLaunch={vi.fn()}
          onDoctor={vi.fn()}
          onOpenQuickInput={vi.fn()}
          onQuickInputChange={vi.fn()}
          onSubmitQuickInput={vi.fn()}
          onCloseQuickInput={vi.fn()}
        />,
      );
    });

    expect(container.textContent).toContain("Stop");
    const input = container.querySelector("input[type='url']") as HTMLInputElement | null;
    expect(input?.value).toBe("https://example.com");
  });
});
