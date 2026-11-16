import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DeviceListItem } from "../DeviceListItem";

describe("DeviceListItem", () => {
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

  it("renders device identity, transport badge, and battery", () => {
    act(() => {
      root.render(
        <DeviceListItem
          title="Pixel 8"
          subtitle='"My phone"'
          telemetryLabel="192.168.1.24:5555"
          status="connected"
          isSelected={true}
          transport="wifi"
          battery={{ level: 78, charging: true }}
          isEditing={false}
          editingName=""
          serial="192.168.1.24:5555"
          hasSession={false}
          isBusy={false}
          showDoctor={false}
          showDisconnect={true}
          canQuickAction={true}
          canForget={false}
          quickAction={null}
          quickInput=""
          quickNote={null}
          onSelect={vi.fn()}
          onStartEdit={vi.fn()}
          onEditingNameChange={vi.fn()}
          onSaveEdit={vi.fn()}
          onCancelEdit={vi.fn()}
          onStop={vi.fn()}
          onLaunch={vi.fn()}
          onDoctor={vi.fn()}
          onDisconnect={vi.fn()}
          onOpenQuickInput={vi.fn()}
          onQuickInputChange={vi.fn()}
          onSubmitQuickInput={vi.fn()}
          onCloseQuickInput={vi.fn()}
        />,
      );
    });

    expect(container.textContent).toContain("Pixel 8");
    expect(container.textContent).toContain('"My phone"');
    expect(container.textContent).toContain("Wireless");
    expect(container.textContent).toContain("78%");
  });
});
