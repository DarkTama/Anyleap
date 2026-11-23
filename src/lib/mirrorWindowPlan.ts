import type { SessionInfo } from "./types";

/** Canonical WebviewWindow label for an embedded mirror window. */
export function mirrorLabelForSerial(serial: string): string {
  return `mirror-${serial.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}

/**
 * Decide whether an exiting session's `session-exited` handler should close the
 * `mirror-<serial>` wrapper window. Returns `false` when a newer session for the
 * same serial is active or an in-place restart is in progress.
 */
export function shouldCloseMirrorWindowOnExit(params: {
  exitedSerial: string;
  activeSessions: readonly SessionInfo[];
  restartingSerials: ReadonlySet<string>;
}): boolean {
  if (!params.exitedSerial) return false;
  if (params.restartingSerials.has(params.exitedSerial)) return false;
  return !params.activeSessions.some((s) => s.serial === params.exitedSerial);
}

/**
 * Decide whether an existing `mirror-*` WebviewWindow should remain open during
 * window reconciliation. Keeps windows for active sessions and serials currently
 * undergoing an in-place restart.
 */
export function shouldKeepMirrorWindowOpen(
  windowLabel: string,
  activeSessions: readonly SessionInfo[],
  restartingSerials: ReadonlySet<string>,
): boolean {
  for (const s of activeSessions) {
    if (mirrorLabelForSerial(s.serial) === windowLabel) {
      return true;
    }
  }
  for (const serial of restartingSerials) {
    if (mirrorLabelForSerial(serial) === windowLabel) {
      return true;
    }
  }
  return false;
}
