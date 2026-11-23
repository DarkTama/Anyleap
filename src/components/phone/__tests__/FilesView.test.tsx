import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "@/store/useAppStore";
import { DEFAULT_TRANSFER_FOLDERS } from "@/lib/transferFolders";
import type { DeviceEntry } from "@/lib/types";

const mockListDeviceDir = vi.fn<(...args: unknown[]) => Promise<DeviceEntry[]>>();
const mockPullDeviceFile = vi.fn((..._args: unknown[]) =>
  Promise.resolve({ savedPath: "C:\\Downloads\\AnyLeap\\notes.txt", fallbackFrom: null as string | null }),
);
const mockOpenDialog = vi.fn((..._args: unknown[]) => Promise.resolve("E:\\Chosen" as string | null));
const mockRevealItemInDir = vi.fn((..._args: unknown[]) => Promise.resolve());

vi.mock("@/lib/tauri", () => ({
  listDeviceDir: (...args: unknown[]) => mockListDeviceDir(...args),
  pullDeviceFile: (...args: unknown[]) => mockPullDeviceFile(...args),
  pushToDevice: vi.fn(async () => {}),
  deleteDevicePath: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => mockOpenDialog(...args),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: (...args: unknown[]) => mockRevealItemInDir(...args),
}));

vi.mock("@tauri-apps/api/path", () => ({
  downloadDir: vi.fn(() => Promise.resolve("C:\\Downloads")),
}));

const mockOnDragDropEvent = vi.fn(async () => () => {});
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: mockOnDragDropEvent,
  }),
}));

import { FilesView } from "../FilesView";

describe("FilesView (SPEC-31)", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
    mockListDeviceDir.mockReset();
    mockListDeviceDir.mockResolvedValue([
      {
        name: "notes.txt",
        path: "/storage/emulated/0/notes.txt",
        is_dir: false,
        size: 128,
        modified_unix: 1_700_000_000,
      },
    ]);
    mockPullDeviceFile.mockClear();
    mockOpenDialog.mockClear();
    mockRevealItemInDir.mockClear();
    mockOnDragDropEvent.mockClear();
    useAppStore.getState().setTransferFolders({ ...DEFAULT_TRANSFER_FOLDERS });

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

  it("downloads without dialog when askWhereToSave is false and offers Show in folder", async () => {
    await act(async () => {
      root.render(<FilesView serial="serial-1" />);
    });

    const checkbox = container.querySelector<HTMLInputElement>('tbody input[type="checkbox"]')!;
    await act(async () => {
      checkbox.click();
    });

    const dlBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Download (1)"),
    )!;
    await act(async () => {
      dlBtn.click();
    });

    expect(mockOpenDialog).not.toHaveBeenCalled();
    expect(mockPullDeviceFile).toHaveBeenCalledWith(
      "serial-1",
      "/storage/emulated/0/notes.txt",
      null,
    );

    const showBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Show in folder"),
    )!;
    expect(showBtn).toBeDefined();
    await act(async () => {
      showBtn.click();
    });
    expect(mockRevealItemInDir).toHaveBeenCalledWith("C:\\Downloads\\AnyLeap\\notes.txt");
  });

  it("prompts when askWhereToSave is true and surfaces fallback notice", async () => {
    useAppStore.getState().setTransferFolders({
      ...DEFAULT_TRANSFER_FOLDERS,
      receiveDir: "Z:\\Missing",
      askWhereToSave: true,
    });
    mockPullDeviceFile.mockResolvedValueOnce({
      savedPath: "C:\\Downloads\\AnyLeap\\notes.txt",
      fallbackFrom: "Z:\\Missing",
    });

    await act(async () => {
      root.render(<FilesView serial="serial-1" />);
    });

    const checkbox = container.querySelector<HTMLInputElement>('tbody input[type="checkbox"]')!;
    await act(async () => {
      checkbox.click();
    });

    const dlBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Download (1)"),
    )!;
    await act(async () => {
      dlBtn.click();
    });

    expect(mockOpenDialog).toHaveBeenCalledWith(
      expect.objectContaining({ directory: true, defaultPath: "Z:\\Missing" }),
    );
    expect(mockPullDeviceFile).toHaveBeenCalledWith(
      "serial-1",
      "/storage/emulated/0/notes.txt",
      "E:\\Chosen",
    );
    expect(container.textContent).toContain('Could not save to "Z:\\Missing"');
  });

  it("shows open folder destination on Upload button and does not register its own drag-drop listener", async () => {
    mockListDeviceDir
      .mockResolvedValueOnce([
        {
          name: "Documents",
          path: "/storage/emulated/0/Documents",
          is_dir: true,
          size: 0,
          modified_unix: 1_700_000_000,
        },
      ])
      .mockResolvedValueOnce([]);

    await act(async () => {
      root.render(<FilesView serial="serial-1" />);
    });

    expect(mockOnDragDropEvent).not.toHaveBeenCalled();

    const uploadBtn = () =>
      Array.from(container.querySelectorAll("button")).find((b) =>
        b.textContent?.includes("Upload to "),
      )!;
    expect(uploadBtn().textContent).toContain("Upload to /storage/emulated/0");
    expect(uploadBtn().getAttribute("title")).toBe("Upload to /storage/emulated/0");

    // Navigate into Documents folder
    const docsCell = Array.from(container.querySelectorAll("td")).find((td) =>
      td.textContent?.includes("Documents"),
    )!;
    await act(async () => {
      docsCell.click();
    });

    expect(uploadBtn().textContent).toContain("Upload to /storage/emulated/0/Documents");
    expect(uploadBtn().getAttribute("title")).toBe("Upload to /storage/emulated/0/Documents");
  });

  it("supports click, Ctrl+click toggle, and Shift+click range selection in displayed order", async () => {
    mockListDeviceDir.mockResolvedValue([
      { name: "a.txt", path: "/storage/emulated/0/a.txt", is_dir: false, size: 10, modified_unix: 1 },
      { name: "b.txt", path: "/storage/emulated/0/b.txt", is_dir: false, size: 20, modified_unix: 2 },
      { name: "c.txt", path: "/storage/emulated/0/c.txt", is_dir: false, size: 30, modified_unix: 3 },
      { name: "d.txt", path: "/storage/emulated/0/d.txt", is_dir: false, size: 40, modified_unix: 4 },
    ]);

    await act(async () => {
      root.render(<FilesView serial="serial-1" />);
    });

    const rowCheckboxes = () =>
      Array.from(container.querySelectorAll<HTMLInputElement>('tbody input[type="checkbox"]'));

    // Click b.txt -> selects only b.txt and sets anchor
    await act(async () => {
      rowCheckboxes()[1].dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(rowCheckboxes().map((c) => c.checked)).toEqual([false, true, false, false]);

    // Shift+click d.txt -> selects b.txt..d.txt
    await act(async () => {
      rowCheckboxes()[3].dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }));
    });
    expect(rowCheckboxes().map((c) => c.checked)).toEqual([false, true, true, true]);

    // Shift+click a.txt -> reverses around anchor b.txt to a.txt..b.txt
    await act(async () => {
      rowCheckboxes()[0].dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }));
    });
    expect(rowCheckboxes().map((c) => c.checked)).toEqual([true, true, false, false]);

    // Ctrl+click d.txt -> toggles d.txt on while keeping a.txt & b.txt
    await act(async () => {
      rowCheckboxes()[3].dispatchEvent(new MouseEvent("click", { bubbles: true, ctrlKey: true }));
    });
    expect(rowCheckboxes().map((c) => c.checked)).toEqual([true, true, false, true]);

    // Ctrl+click b.txt -> toggles b.txt off
    await act(async () => {
      rowCheckboxes()[1].dispatchEvent(new MouseEvent("click", { bubbles: true, ctrlKey: true }));
    });
    expect(rowCheckboxes().map((c) => c.checked)).toEqual([true, false, false, true]);

    // Plain click on c.txt row -> collapses selection to c.txt
    const rows = Array.from(container.querySelectorAll("tbody tr"));
    await act(async () => {
      rows[2].dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(rowCheckboxes().map((c) => c.checked)).toEqual([false, false, true, false]);
  });
});
