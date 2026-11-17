import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { LaunchMatrix } from "../LaunchMatrix";
import { useAppStore } from "@/store/useAppStore";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("LaunchMatrix noAudio", () => {
  for (const pre of [false, true]) {
    it(`camera then mirror/flex keeps noAudio=${pre}`, async () => {
      useAppStore.getState().setSettings({ ...useAppStore.getState().settings, noAudio: pre });
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
      expect(useAppStore.getState().settings.noAudio).toBe(pre);
      await click("Phone Mirror");
      expect(useAppStore.getState().settings.noAudio).toBe(pre);
      await click("Desktop Flex");
      expect(useAppStore.getState().settings.noAudio).toBe(pre);
      expect(picked).toEqual(["camera", "mirror", "flex"]);
      await act(async () => root.unmount());
    });
  }
});
