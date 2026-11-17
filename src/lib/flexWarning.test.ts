import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-store", () => ({ load: vi.fn() }));

import { FLEX_SECURE_WARNING_KEY, takeFlexSecureWarning } from "./flexWarning";
import type { KeyValueStore } from "./settingsStore";

function fakeStore(initial: Record<string, unknown> = {}) {
  const data = { ...initial };
  const store: KeyValueStore = {
    get: async <T,>(k: string) => data[k] as T,
    set: async (k, v) => {
      data[k] = v;
    },
    save: async () => {},
  };
  return { store, data };
}

describe("takeFlexSecureWarning", () => {
  it("shows once, then never again", async () => {
    const { store, data } = fakeStore();
    expect(await takeFlexSecureWarning(store)).toBe(true);
    expect(data[FLEX_SECURE_WARNING_KEY]).toBe(true);
    expect(await takeFlexSecureWarning(store)).toBe(false);
  });
  it("returns when the flag is cleared", async () => {
    const { store, data } = fakeStore({ [FLEX_SECURE_WARNING_KEY]: true });
    expect(await takeFlexSecureWarning(store)).toBe(false);
    data[FLEX_SECURE_WARNING_KEY] = false;
    expect(await takeFlexSecureWarning(store)).toBe(true);
  });
});
