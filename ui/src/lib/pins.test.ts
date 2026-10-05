import { describe, expect, it } from "vitest";

import { dropGap, dropIndex, samePin } from "./pins";

describe("samePin", () => {
  it("ignores the label, so a rename is not a second pin", () => {
    expect(
      samePin({ kind: "file", target: "a", label: "old" }, { kind: "file", target: "a", label: "new" }),
    ).toBe(true);
  });

  it("separates two kinds sharing a target string", () => {
    expect(samePin({ kind: "file", target: "x", label: "" }, { kind: "folder", target: "x", label: "" })).toBe(false);
  });
});

describe("dragging a pin to a new position", () => {
  // Four chips, 20px apart, so the midpoints are easy to reason about.
  const mids = [10, 30, 50, 70];

  it("picks the gap by which midpoints the pointer has passed", () => {
    expect(dropGap(mids, 0)).toBe(0); // before everything
    expect(dropGap(mids, 20)).toBe(1);
    expect(dropGap(mids, 60)).toBe(3);
    expect(dropGap(mids, 999)).toBe(4); // past the end
  });

  it("does not move a chip until the pointer clears a neighbour's midpoint", () => {
    // Still inside its own slot, and inside the left half of the next one.
    expect(dropIndex(mids, 15, 0)).toBe(0);
    expect(dropIndex(mids, 29, 0)).toBe(0);
    // Past the second chip's midpoint: now they swap.
    expect(dropIndex(mids, 31, 0)).toBe(1);
  });

  it("discounts the dragged chip when moving right, but not when moving left", () => {
    // Dragging the first chip to the far right lands at the last index, not
    // past the end — the count still included the chip being carried.
    expect(dropIndex(mids, 999, 0)).toBe(3);
    // Dragging the last chip to the far left is a plain insert at zero.
    expect(dropIndex(mids, 0, 3)).toBe(0);
  });

  it("reports the chip's own index anywhere the drop would change nothing", () => {
    // Either side of chip 2's own midpoint is still chip 2's place.
    expect(dropIndex(mids, 45, 2)).toBe(2);
    expect(dropIndex(mids, 55, 2)).toBe(2);
  });

  it("never lands outside the strip, wherever the pointer goes", () => {
    for (const from of [0, 1, 2, 3]) {
      for (const x of [-500, 0, 25, 50, 75, 5000]) {
        const index = dropIndex(mids, x, from);
        expect(index).toBeGreaterThanOrEqual(0);
        expect(index).toBeLessThan(mids.length);
      }
    }
  });

  it("handles a strip of one, where every drop is a no-op", () => {
    expect(dropIndex([10], -100, 0)).toBe(0);
    expect(dropIndex([10], 100, 0)).toBe(0);
  });
});
