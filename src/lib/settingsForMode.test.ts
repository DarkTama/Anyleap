import { describe, expect, it } from "vitest";
import { settingsForMode, type SettingsMode } from "./settingsForMode";
import type { CoreSettings } from "@/lib/types";

const modes: SettingsMode[] = ["flex", "mirror", "camera", "hifi"];

describe("settingsForMode", () => {
  for (const pre of [false, true]) {
    it(`never changes noAudio (pre=${pre}) across camera -> mirror/flex`, () => {
      const base = { noAudio: pre } as CoreSettings;
      for (const m of modes) {
        expect(settingsForMode(base, m).noAudio).toBe(pre);
      }
      const cam = settingsForMode(base, "camera");
      expect(settingsForMode(cam, "mirror").noAudio).toBe(pre);
      expect(settingsForMode(cam, "flex").noAudio).toBe(pre);
    });
  }
});
