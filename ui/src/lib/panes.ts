/**
 * The split layout, as a tree: what `:sp` and `:vsp` build.
 *
 * A leaf is a pane, with a strip of tabs of its own. A split sets its
 * children side by side (`row`) or one above the other (`column`), each with
 * a share of the room. Everything here is arithmetic on that tree; what is
 * drawn from it, and what the tabs are, is somebody else's business.
 *
 * The tree is changed in place, and the functions that can replace the root
 * hand the new one back.
 */

export type Leaf = {
  kind: "leaf";
  id: number;
  /** Files (above zero) and terminals (below), in the order of the strip. */
  tabs: number[];
  active: number | null;
  /** The tab that was showing before `active`, to go back to. */
  last: number | null;
};

export type Split = {
  kind: "split";
  id: number;
  dir: SplitDir;
  children: Node[];
  /** Each child's share of the split, adding up to one. */
  sizes: number[];
};

export type Node = Leaf | Split;
/** `row` is side by side, which is what `:vsp` makes; `column` is `:sp`. */
export type SplitDir = "row" | "column";
export type Direction = "left" | "right" | "up" | "down";

/** A box within the stage, in fractions of its width and height. */
export type Rect = { x: number; y: number; w: number; h: number };

/** The line between two children of a split, which is dragged to resize them. */
export type Divider = { split: number; index: number; dir: SplitDir; x: number; y: number; length: number };

const WHOLE: Rect = { x: 0, y: 0, w: 1, h: 1 };
/** The least of the stage a pane can be dragged down to. */
const MIN_SHARE = 0.08;
const EPSILON = 1e-6;

export function leaves(node: Node): Leaf[] {
  return node.kind === "leaf" ? [node] : node.children.flatMap(leaves);
}

function find(node: Node, id: number): Node | null {
  if (node.id === id) return node;
  if (node.kind === "leaf") return null;
  for (const child of node.children) {
    const found = find(child, id);
    if (found) return found;
  }
  return null;
}

export function splitOf(root: Node, id: number): Split | null {
  const found = find(root, id);
  return found?.kind === "split" ? found : null;
}

function parentOf(root: Node, id: number): Split | null {
  if (root.kind === "leaf") return null;
  for (const child of root.children) {
    if (child.id === id) return root;
    const deeper = parentOf(child, id);
    if (deeper) return deeper;
  }
  return null;
}

/** Where every node is, splits included, by id. */
export function rects(node: Node, into = new Map<number, Rect>(), box = WHOLE): Map<number, Rect> {
  into.set(node.id, box);
  if (node.kind === "leaf") return into;

  let at = 0;
  node.children.forEach((child, index) => {
    const share = node.sizes[index];
    rects(
      child,
      into,
      node.dir === "row"
        ? { x: box.x + box.w * at, y: box.y, w: box.w * share, h: box.h }
        : { x: box.x, y: box.y + box.h * at, w: box.w, h: box.h * share },
    );
    at += share;
  });
  return into;
}

export function dividers(root: Node): Divider[] {
  const boxes = rects(root);
  const lines: Divider[] = [];
  const walk = (node: Node) => {
    if (node.kind === "leaf") return;
    const box = boxes.get(node.id)!;
    let at = 0;
    for (let index = 0; index < node.children.length - 1; index++) {
      at += node.sizes[index];
      lines.push(
        node.dir === "row"
          ? { split: node.id, index, dir: "row", x: box.x + box.w * at, y: box.y, length: box.h }
          : { split: node.id, index, dir: "column", x: box.x, y: box.y + box.h * at, length: box.w },
      );
    }
    node.children.forEach(walk);
  };
  walk(root);
  return lines;
}

/**
 * Put `fresh` beside the pane `id`: to its right for a row, below it for a
 * column. It takes half of that pane's room, and nobody else's.
 */
export function split(root: Node, id: number, dir: SplitDir, fresh: Leaf, splitId: number): Node {
  const target = find(root, id);
  if (!target) return root;
  const parent = parentOf(root, id);

  // Already one of a run in this direction: it joins the run.
  if (parent && parent.dir === dir) {
    const at = parent.children.findIndex((child) => child.id === id);
    const half = parent.sizes[at] / 2;
    parent.sizes.splice(at, 1, half, half);
    parent.children.splice(at + 1, 0, fresh);
    return root;
  }

  const made: Split = { kind: "split", id: splitId, dir, children: [target, fresh], sizes: [0.5, 0.5] };
  if (!parent) return made;
  parent.children[parent.children.findIndex((child) => child.id === id)] = made;
  return root;
}

/** Take a pane out. Its neighbours share out the room; the last one stays. */
export function remove(root: Node, id: number): Node {
  const parent = parentOf(root, id);
  if (!parent) return root;

  const at = parent.children.findIndex((child) => child.id === id);
  parent.children.splice(at, 1);
  parent.sizes.splice(at, 1);
  const total = parent.sizes.reduce((sum, share) => sum + share, 0);
  parent.sizes = parent.sizes.map((share) => share / total);
  if (parent.children.length > 1) return root;

  // A split of one is not a split: what is left takes its place.
  const only = parent.children[0];
  const grand = parentOf(root, parent.id);
  if (!grand) return only;

  const slot = grand.children.findIndex((child) => child.id === parent.id);
  if (only.kind === "split" && only.dir === grand.dir) {
    // And a run inside a run of the same direction is one run.
    const share = grand.sizes[slot];
    grand.children.splice(slot, 1, ...only.children);
    grand.sizes.splice(slot, 1, ...only.sizes.map((size) => size * share));
  } else {
    grand.children[slot] = only;
  }
  return root;
}

/**
 * Drag the line after child `index` of a split to `at`, a fraction of the
 * whole stage along the split's direction. Only the two panes either side of
 * the line change size.
 */
export function resize(node: Split, box: Rect, index: number, at: number) {
  const start = node.dir === "row" ? box.x : box.y;
  const span = node.dir === "row" ? box.w : box.h;
  if (span <= 0) return;

  const before = node.sizes.slice(0, index).reduce((sum, share) => sum + share, 0);
  const pair = node.sizes[index] + node.sizes[index + 1];
  const least = Math.min(MIN_SHARE / span, pair / 2);
  const first = Math.min(pair - least, Math.max(least, (at - start) / span - before));
  node.sizes[index] = first;
  node.sizes[index + 1] = pair - first;
}

/**
 * The pane next to `id` on one side: the one sharing the most of that edge,
 * which is the one the eye says is "the pane to the left".
 */
export function neighbour(root: Node, id: number, towards: Direction): number | null {
  const boxes = rects(root);
  const from = boxes.get(id);
  if (!from) return null;
  const near = (a: number, b: number) => Math.abs(a - b) < EPSILON;

  let best: number | null = null;
  let most = EPSILON;
  for (const leaf of leaves(root)) {
    if (leaf.id === id) continue;
    const box = boxes.get(leaf.id)!;
    const touching =
      towards === "left"
        ? near(box.x + box.w, from.x)
        : towards === "right"
          ? near(box.x, from.x + from.w)
          : towards === "up"
            ? near(box.y + box.h, from.y)
            : near(box.y, from.y + from.h);
    if (!touching) continue;

    const shared =
      towards === "left" || towards === "right"
        ? Math.min(box.y + box.h, from.y + from.h) - Math.max(box.y, from.y)
        : Math.min(box.x + box.w, from.x + from.w) - Math.max(box.x, from.x);
    if (shared > most) {
      best = leaf.id;
      most = shared;
    }
  }
  return best;
}

// --- the layout, written down ---------------------------------------------------

/** A layout with its tabs named by something that outlives the window. */
export type SavedNode =
  | { tabs: string[]; active: string | null }
  | { dir: SplitDir; sizes: number[]; children: SavedNode[] };

/** The tree with each tab named by `name`; tabs it has no name for are left out. */
export function freeze(node: Node, name: (key: number) => string | null): SavedNode {
  if (node.kind === "split") {
    return {
      dir: node.dir,
      sizes: [...node.sizes],
      children: node.children.map((child) => freeze(child, name)),
    };
  }
  return {
    tabs: node.tabs.flatMap((key) => name(key) ?? []),
    active: node.active === null ? null : name(node.active),
  };
}

/**
 * The tree a saved layout describes, or null if nothing of it is left: panes
 * none of whose tabs can be found are dropped, and the splits close up round
 * them. Never trusts what it is given, which was read from a file.
 */
export function revive(
  saved: unknown,
  key: (name: string) => number | null,
  nextId: () => number,
): Node | null {
  if (saved === null || typeof saved !== "object") return null;
  const node = saved as Record<string, unknown>;

  if (Array.isArray(node.tabs)) {
    const tabs = [
      ...new Set(node.tabs.flatMap((name) => (typeof name === "string" ? (key(name) ?? []) : []))),
    ];
    if (tabs.length === 0) return null;
    const active = typeof node.active === "string" ? key(node.active) : null;
    return {
      kind: "leaf",
      id: nextId(),
      tabs,
      active: active !== null && tabs.includes(active) ? active : tabs[0],
      last: null,
    };
  }

  if (!Array.isArray(node.children) || (node.dir !== "row" && node.dir !== "column")) return null;
  const listed: unknown[] = node.children;
  const children: Node[] = [];
  const sizes: number[] = [];
  listed.forEach((child, index) => {
    const made = revive(child, key, nextId);
    if (!made) return;
    const share = Array.isArray(node.sizes) ? Number(node.sizes[index]) : NaN;
    children.push(made);
    sizes.push(share > 0 && share <= 1 ? share : 1 / listed.length);
  });
  if (children.length === 0) return null;
  if (children.length === 1) return children[0];

  const total = sizes.reduce((sum, share) => sum + share, 0);
  return {
    kind: "split",
    id: nextId(),
    dir: node.dir,
    children,
    sizes: sizes.map((share) => share / total),
  };
}
