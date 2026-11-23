import { describe, expect, it } from "vitest";
import {
  mirrorLabelForSerial,
  shouldCloseMirrorWindowOnExit,
  shouldKeepMirrorWindowOpen,
} from "./mirrorWindowPlan";
import type { SessionInfo } from "./types";

const makeSession = (id: string, serial: string): SessionInfo => ({
  id,
  serial,
  pid: 1000,
  started_at: 1,
  mode: "display",
  recording_path: null,
});

describe("mirrorWindowPlan", () => {
  it("sanitizes wireless serials identically to the Rust backend", () => {
    expect(mirrorLabelForSerial("192.168.1.24:36431")).toBe("mirror-192_168_1_24_36431");
    expect(mirrorLabelForSerial("ABC-123_xyz")).toBe("mirror-ABC-123_xyz");
  });

  it("does not close the mirror window when a replacement session is already active or restarting", () => {
    const newSession = makeSession("s2", "192.168.1.24:36431");

    expect(
      shouldCloseMirrorWindowOnExit({
        exitedSerial: "192.168.1.24:36431",
        activeSessions: [newSession],
        restartingSerials: new Set(),
      }),
    ).toBe(false);

    expect(
      shouldCloseMirrorWindowOnExit({
        exitedSerial: "192.168.1.24:36431",
        activeSessions: [],
        restartingSerials: new Set(["192.168.1.24:36431"]),
      }),
    ).toBe(false);

    expect(
      shouldCloseMirrorWindowOnExit({
        exitedSerial: "192.168.1.24:36431",
        activeSessions: [],
        restartingSerials: new Set(),
      }),
    ).toBe(true);
  });

  it("keeps mirror windows open during active sessions and in-place restarts", () => {
    const label = mirrorLabelForSerial("192.168.1.24:36431");
    expect(shouldKeepMirrorWindowOpen(label, [makeSession("s1", "192.168.1.24:36431")], new Set())).toBe(true);
    expect(shouldKeepMirrorWindowOpen(label, [], new Set(["192.168.1.24:36431"]))).toBe(true);
    expect(shouldKeepMirrorWindowOpen(label, [], new Set())).toBe(false);
  });
});
