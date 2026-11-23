export interface RangeSelectionState {
  selected: Set<string>;
  anchorId: string | null;
}

export interface RangeSelectionModifiers {
  shiftKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  /** When true, plain-clicking the sole selected item clears the selection (checkbox toggle). */
  toggleIfSole?: boolean;
}

export function emptyRangeSelection(): RangeSelectionState {
  return { selected: new Set(), anchorId: null };
}

/**
 * Compute next selection state in list/grid display order:
 * - Click selects the item and sets it as the anchor (or clears if `toggleIfSole` and it is the only selected item).
 * - Ctrl/Cmd+click toggles the item and updates the anchor.
 * - Shift+click selects the contiguous range between the last anchor and the clicked item, preserving the anchor.
 */
export function applyRangeSelection(
  orderedIds: readonly string[],
  state: RangeSelectionState,
  targetId: string,
  modifiers: RangeSelectionModifiers = {},
): RangeSelectionState {
  const targetIdx = orderedIds.indexOf(targetId);
  if (targetIdx === -1) return state;

  const ctrlOrMeta = Boolean(modifiers.ctrlKey || modifiers.metaKey);
  const shift = Boolean(modifiers.shiftKey);

  if (shift) {
    const anchorIdx =
      state.anchorId !== null ? orderedIds.indexOf(state.anchorId) : -1;
    if (anchorIdx === -1) {
      const next = ctrlOrMeta ? new Set(state.selected) : new Set<string>();
      next.add(targetId);
      return { selected: next, anchorId: targetId };
    }
    const start = Math.min(anchorIdx, targetIdx);
    const end = Math.max(anchorIdx, targetIdx);
    const next = ctrlOrMeta ? new Set(state.selected) : new Set<string>();
    for (let i = start; i <= end; i += 1) {
      next.add(orderedIds[i]);
    }
    return { selected: next, anchorId: state.anchorId };
  }

  if (ctrlOrMeta) {
    const next = new Set(state.selected);
    let nextAnchor: string | null;
    if (next.has(targetId)) {
      next.delete(targetId);
      if (state.anchorId === targetId) {
        const remaining = Array.from(next);
        nextAnchor = remaining.length > 0 ? remaining[remaining.length - 1] : null;
      } else {
        nextAnchor = state.anchorId;
      }
    } else {
      next.add(targetId);
      nextAnchor = targetId;
    }
    return { selected: next, anchorId: nextAnchor };
  }

  if (
    modifiers.toggleIfSole &&
    state.selected.size === 1 &&
    state.selected.has(targetId)
  ) {
    return emptyRangeSelection();
  }

  return {
    selected: new Set([targetId]),
    anchorId: targetId,
  };
}
