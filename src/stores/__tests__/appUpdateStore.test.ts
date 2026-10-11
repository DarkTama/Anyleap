import { describe, it, expect, beforeEach, vi } from "vitest";
import { useAppUpdateStore } from "../appUpdateStore";

describe("appUpdateStore", () => {
  beforeEach(() => {
    useAppUpdateStore.getState().reset();
    vi.restoreAllMocks();
  });

  it("starts in idle state", () => {
    const state = useAppUpdateStore.getState();
    expect(state.phase).toBe("idle");
    expect(state.progress).toBeNull();
    expect(state.error).toBeNull();
    expect(state.hasUpdate).toBe(false);
  });

  it("resets state properly", () => {
    useAppUpdateStore.setState({ phase: "error", error: "Download failed", hasUpdate: true });
    useAppUpdateStore.getState().reset();
    const state = useAppUpdateStore.getState();
    expect(state.phase).toBe("idle");
    expect(state.error).toBeNull();
    expect(state.hasUpdate).toBe(false);
  });

  it("updates state on manual dismiss", () => {
    useAppUpdateStore.setState({ hasUpdate: true, availableVersion: "0.9.7" });
    useAppUpdateStore.getState().dismissUpdate();
    expect(useAppUpdateStore.getState().dismissed).toBe(true);
  });
});
