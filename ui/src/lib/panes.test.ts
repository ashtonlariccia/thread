import { describe, expect, it } from "vitest";

import {
  dividers,
  freeze,
  leaves,
  neighbour,
  rects,
  remove,
  resize,
  revive,
  split,
  splitOf,
  type Leaf,
  type Node,
} from "./panes";

const leaf = (id: number, tabs: number[] = []): Leaf => ({
  kind: "leaf",
  id,
  tabs,
  active: tabs[0] ?? null,
  last: null,
});

/** 1 | (2 over 3): `:vsp`, then `:sp` in the right-hand pane. */
function three(): Node {
  let root: Node = leaf(1);
  root = split(root, 1, "row", leaf(2), 10);
  return split(root, 2, "column", leaf(3), 11);
}

describe("splitting", () => {
  it("halves the pane that was split and leaves the others alone", () => {
    let root: Node = leaf(1);
    root = split(root, 1, "row", leaf(2), 10);
    root = split(root, 2, "row", leaf(3), 11);

    // One run of three, not a split inside a split.
    expect(root.kind === "split" && root.children.map((c) => c.id)).toEqual([1, 2, 3]);
    expect(root.kind === "split" && root.sizes).toEqual([0.5, 0.25, 0.25]);
  });

  it("nests when the direction changes", () => {
    const boxes = rects(three());
    expect(boxes.get(1)).toEqual({ x: 0, y: 0, w: 0.5, h: 1 });
    expect(boxes.get(2)).toEqual({ x: 0.5, y: 0, w: 0.5, h: 0.5 });
    expect(boxes.get(3)).toEqual({ x: 0.5, y: 0.5, w: 0.5, h: 0.5 });
  });

  it("draws a line for every boundary", () => {
    expect(dividers(three())).toEqual([
      { split: 10, index: 0, dir: "row", x: 0.5, y: 0, length: 1 },
      { split: 11, index: 0, dir: "column", x: 0.5, y: 0.5, length: 0.5 },
    ]);
  });
});

describe("closing a pane", () => {
  it("gives its room to what is left, and collapses a split of one", () => {
    const root = remove(three(), 3);
    expect(leaves(root).map((l) => l.id)).toEqual([1, 2]);
    expect(rects(root).get(2)).toEqual({ x: 0.5, y: 0, w: 0.5, h: 1 });
  });

  it("leaves the last pane where it is", () => {
    const only = leaf(1);
    expect(remove(only, 1)).toBe(only);
  });

  it("joins a run that ends up inside a run of the same direction", () => {
    // (1 over 2) | 3, with 2 itself split sideways into 2 | 4.
    let root: Node = leaf(1);
    root = split(root, 1, "row", leaf(3), 10);
    root = split(root, 1, "column", leaf(2), 11);
    root = split(root, 2, "row", leaf(4), 12);
    root = remove(root, 1);

    expect(root.kind === "split" && root.children.map((c) => c.id)).toEqual([2, 4, 3]);
    expect(root.kind === "split" && root.sizes).toEqual([0.25, 0.25, 0.5]);
  });
});

describe("moving between panes", () => {
  it("finds the pane across each edge", () => {
    const root = three();
    expect(neighbour(root, 1, "right")).toBe(2);
    expect(neighbour(root, 2, "left")).toBe(1);
    expect(neighbour(root, 2, "down")).toBe(3);
    expect(neighbour(root, 3, "up")).toBe(2);
    expect(neighbour(root, 3, "left")).toBe(1);
  });

  it("has nowhere to go at the edge of the window", () => {
    const root = three();
    expect(neighbour(root, 1, "left")).toBeNull();
    expect(neighbour(root, 2, "up")).toBeNull();
  });
});

describe("dragging a divider", () => {
  it("moves room between the two panes beside it", () => {
    const root = three();
    const node = splitOf(root, 10)!;
    resize(node, rects(root).get(10)!, 0, 0.3);
    expect(node.sizes[0]).toBeCloseTo(0.3);
    expect(node.sizes[1]).toBeCloseTo(0.7);
  });

  it("stops before a pane is squeezed to nothing", () => {
    const root = three();
    const node = splitOf(root, 10)!;
    resize(node, rects(root).get(10)!, 0, 5);
    expect(node.sizes[1]).toBeGreaterThan(0.05);
    expect(node.sizes[0] + node.sizes[1]).toBeCloseTo(1);
  });
});

describe("a saved layout", () => {
  const names: Record<number, string> = { 1: "a.rs", 2: "b.rs", 3: "c.rs" };
  const name = (key: number) => names[key] ?? null;
  const keyed = (wanted: string) => {
    const found = Object.entries(names).find(([, n]) => n === wanted);
    return found ? Number(found[0]) : null;
  };

  it("comes back as it was", () => {
    let root: Node = leaf(1, [1, 2]);
    root = split(root, 1, "row", leaf(2, [3, -1]), 10);
    const saved = freeze(root, name);
    // The terminal has no name, and is not written down.
    expect(saved).toEqual({
      dir: "row",
      sizes: [0.5, 0.5],
      children: [
        { tabs: ["a.rs", "b.rs"], active: "a.rs" },
        { tabs: ["c.rs"], active: "c.rs" },
      ],
    });

    let id = 20;
    const back = revive(saved, keyed, () => id++)!;
    expect(leaves(back).map((l) => l.tabs)).toEqual([[1, 2], [3]]);
  });

  it("closes up round a pane whose files have gone", () => {
    const saved = {
      dir: "row",
      sizes: [0.5, 0.5],
      children: [
        { tabs: ["a.rs"], active: "a.rs" },
        { tabs: ["gone.rs"], active: "gone.rs" },
      ],
    };
    let id = 20;
    const back = revive(saved, keyed, () => id++)!;
    expect(back.kind).toBe("leaf");
  });

  it("is nothing at all when it is not a layout", () => {
    expect(revive("nonsense", keyed, () => 1)).toBeNull();
    expect(revive({ dir: "diagonal", children: [] }, keyed, () => 1)).toBeNull();
  });
});
