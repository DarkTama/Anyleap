import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it } from "vitest";
import { LaunchMatrix } from "../LaunchMatrix";
import { useAppStore } from "@/store/useAppStore";
import { defaultModeDefaults, resolveLaunchSettings } from "@/lib/modeSettings";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("LaunchMatrix", () => {
  beforeEach(() => {
    useAppStore.setState({ modeDefaults: defaultModeDefaults(), currentMode: "mirror" });
  });

  for (const noAudio of [false, true]) {
    it(`camera then mirror/flex leaves every mode untouched (mirror noAudio=${noAudio})`, async () => {
      useAppStore.getState().updateMode("mirror", { noAudio });
      const before = structuredClone(useAppStore.getState().modeDefaults);
      const picked: string[] = [];
      const el = document.createElement("div");
      document.body.appendChild(el);
      const root = createRoot(el);
      await act(async () => {
        root.render(<LaunchMatrix currentMode="mirror" onSelectMode={(m) => picked.push(m)} />);
      });
      const click = async (title: string) => {
        const btn = Array.from(el.querySelectorAll("button")).find((b) =>
          b.textContent?.includes(title),
        );
        expect(btn).toBeTruthy();
        await act(async () => {
          btn!.click();
        });
      };
      await click("Camera");
      await click("Phone Mirror");
      await click("Desktop Flex");
      await click("High Fidelity");
      expect(picked).toEqual(["camera", "mirror", "flex", "hifi"]);
      // Selecting a mode writes no settings (SPEC-28 §5.1 rule 3).
      expect(useAppStore.getState().modeDefaults).toEqual(before);
      expect(
        resolveLaunchSettings("mirror", useAppStore.getState().modeDefaults).noAudio,
      ).toBe(noAudio);
      await act(async () => root.unmount());
    });
  }
});
