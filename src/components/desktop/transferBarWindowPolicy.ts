export type TransferBarWindowAction = "open" | "reveal" | "recreate" | "close" | "none";

export interface TransferBarWindowDecisionInput {
  hasContent: boolean;
  hadContent: boolean;
  windowExists: boolean;
}

/**
 * Decide what the main window must do with the floating bar window.
 * A window that exists while the bar was previously empty is stale (it is
 * hidden, empty, or mid self-close), so it is replaced rather than trusted.
 */
export function decideTransferBarWindowAction(
  input: TransferBarWindowDecisionInput,
): TransferBarWindowAction {
  if (input.hasContent) {
    if (!input.windowExists) return "open";
    return input.hadContent ? "reveal" : "recreate";
  }
  if (input.hadContent && input.windowExists) return "close";
  return "none";
}

export const TRANSFER_BAR_CLOSE_WAIT_MS = 1500;
export const TRANSFER_BAR_REVEAL_VERIFY_MS = 350;
