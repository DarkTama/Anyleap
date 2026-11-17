import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import { phoneOpenApp } from "./phoneHubService";
import { launchAppWindow } from "@/lib/tauri";
import { useAppStore } from "@/store/useAppStore";
import { defaultModeDefaults, resolveLaunchSettings } from "@/lib/modeSettings";

function setup(mode: "mirror" | "flex" | "camera" | "hifi") {
  const d = defaultModeDefaults();
  d.mirror = { ...d.mirror, noAudio: false };
  d.flex = { ...d.flex, noAudio: true };
  useAppStore.setState({ currentMode: mode, modeDefaults: d });
  return d;
}

describe("open-app new-window settings", () => {
  beforeEach(() => invokeMock.mockReset().mockResolvedValue({ kind: "newWindow" }));

  it.each([
    ["mirror", "mirror"],
    ["flex", "flex"],
    ["camera", "mirror"],
    ["hifi", "hifi"],
  ] as const)("selected %s resolves %s settings", async (selected, resolved) => {
    const d = setup(selected);
    const expected = resolveLaunchSettings(resolved, d);
    await phoneOpenApp("s1", "com.x");
    expect(invokeMock).toHaveBeenLastCalledWith("phone_open_app", {
      serial: "s1",
      package: "com.x",
      settings: expected,
    });
    await launchAppWindow("s1", "com.x");
    expect(invokeMock).toHaveBeenLastCalledWith("launch_app_window", {
      serial: "s1",
      package: "com.x",
      settings: expected,
    });
  });
});
