import { describe, expect, it } from "vitest";
import {
  applyRangeSelection,
  emptyRangeSelection,
  type RangeSelectionState,
} from "./rangeSelection";

const IDS = ["a", "b", "c", "d", "e"];

describe("applyRangeSelection", () => {
  it("plain click selects only the clicked item and sets anchor", () => {
    let state: RangeSelectionState = emptyRangeSelection();
    state = applyRangeSelection(IDS, state, "b");
    expect([...state.selected]).toEqual(["b"]);
    expect(state.anchorId).toBe("b");

    state = applyRangeSelection(IDS, state, "d");
    expect([...state.selected]).toEqual(["d"]);
    expect(state.anchorId).toBe("d");
  });

  it("plain click with toggleIfSole clears when clicking the sole selected item", () => {
    let state = applyRangeSelection(IDS, emptyRangeSelection(), "b", {
      toggleIfSole: true,
    });
    expect([...state.selected]).toEqual(["b"]);

    state = applyRangeSelection(IDS, state, "b", { toggleIfSole: true });
    expect(state.selected.size).toBe(0);
    expect(state.anchorId).toBeNull();

    // When multiple are selected, plain click with toggleIfSole still collapses to the clicked item.
    state = applyRangeSelection(IDS, state, "a");
    state = applyRangeSelection(IDS, state, "c", { shiftKey: true });
    expect([...state.selected]).toEqual(["a", "b", "c"]);
    state = applyRangeSelection(IDS, state, "b", { toggleIfSole: true });
    expect([...state.selected]).toEqual(["b"]);
    expect(state.anchorId).toBe("b");
  });

  it("Ctrl+click and Meta+click toggle individual items", () => {
    let state = applyRangeSelection(IDS, emptyRangeSelection(), "a");
    state = applyRangeSelection(IDS, state, "d", { ctrlKey: true });
    expect([...state.selected]).toEqual(["a", "d"]);
    expect(state.anchorId).toBe("d");

    state = applyRangeSelection(IDS, state, "e", { metaKey: true });
    expect([...state.selected]).toEqual(["a", "d", "e"]);
    expect(state.anchorId).toBe("e");

    state = applyRangeSelection(IDS, state, "d", { ctrlKey: true });
    expect([...state.selected]).toEqual(["a", "e"]);
  });

  it("Shift+click selects forward and backward ranges from the last anchor", () => {
    let state = applyRangeSelection(IDS, emptyRangeSelection(), "b");
    state = applyRangeSelection(IDS, state, "e", { shiftKey: true });
    expect([...state.selected]).toEqual(["b", "c", "d", "e"]);
    expect(state.anchorId).toBe("b");

    // Repeated Shift+click shrinks/reverses around the same anchor
    state = applyRangeSelection(IDS, state, "c", { shiftKey: true });
    expect([...state.selected]).toEqual(["b", "c"]);
    expect(state.anchorId).toBe("b");

    state = applyRangeSelection(IDS, state, "a", { shiftKey: true });
    expect([...state.selected]).toEqual(["a", "b"]);
    expect(state.anchorId).toBe("b");
  });

  it("Shift+click with no or stale anchor selects the clicked item and anchors there", () => {
    const noAnchor = applyRangeSelection(IDS, emptyRangeSelection(), "c", {
      shiftKey: true,
    });
    expect([...noAnchor.selected]).toEqual(["c"]);
    expect(noAnchor.anchorId).toBe("c");

    const staleAnchor = applyRangeSelection(
      IDS,
      { selected: new Set(["missing"]), anchorId: "missing" },
      "d",
      { shiftKey: true },
    );
    expect([...staleAnchor.selected]).toEqual(["d"]);
    expect(staleAnchor.anchorId).toBe("d");
  });

  it("Ctrl+Shift+click unions the range with existing selection", () => {
    let state = applyRangeSelection(IDS, emptyRangeSelection(), "a");
    state = applyRangeSelection(IDS, state, "c", { ctrlKey: true });
    state = applyRangeSelection(IDS, state, "e", {
      ctrlKey: true,
      shiftKey: true,
    });
    expect([...state.selected]).toEqual(["a", "c", "d", "e"]);
    expect(state.anchorId).toBe("c");
  });

  it("ignores unknown target IDs", () => {
    const initial = applyRangeSelection(IDS, emptyRangeSelection(), "b");
    expect(applyRangeSelection(IDS, initial, "z", { shiftKey: true })).toBe(
      initial,
    );
  });
});
