/**
 * The panes of this window, and which tabs are in each.
 *
 * One pane to begin with. `:sp`, `:vsp` and the buttons on a strip make more,
 * and a pane whose last tab is closed goes again. A file can be in several
 * panes at once, each a view of the same text; a terminal is in exactly one.
 *
 * This knows tabs only as numbers: files above zero, terminals below. What
 * they are is `Documents`' and App's business.
 */
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
  type Direction,
  type Leaf,
  type Node,
  type SavedNode,
  type SplitDir,
} from "../panes";

export class Layout {
  #ids = 1;

  root = $state<Node>(this.#leaf());
  /** The pane the keyboard is in, and the one the menus act on. */
  focused = $state(1);

  readonly leaves = $derived(leaves(this.root));
  /** Where each pane and each split is, in fractions of the stage. */
  readonly boxes = $derived(rects(this.root));
  readonly dividers = $derived(dividers(this.root));

  #leaf(tabs: number[] = []): Leaf {
    return { kind: "leaf", id: this.#ids++, tabs, active: tabs.at(-1) ?? null, last: null };
  }

  leaf(id: number): Leaf | null {
    return this.leaves.find((leaf) => leaf.id === id) ?? null;
  }

  get current(): Leaf {
    return this.leaf(this.focused) ?? this.leaves[0];
  }

  /** The file showing in the focused pane; null if that is a terminal, or nothing. */
  get activeFile(): number | null {
    const active = this.current.active;
    return active !== null && active > 0 ? active : null;
  }

  /** Every pane with this tab in it. */
  holding(key: number): Leaf[] {
    return this.leaves.filter((leaf) => leaf.tabs.includes(key));
  }

  focus(id: number) {
    if (this.focused !== id && this.leaf(id)) this.focused = id;
  }

  #activate(leaf: Leaf, key: number) {
    if (leaf.active === key) return;
    leaf.last = leaf.active;
    leaf.active = key;
  }

  /**
   * Bring a tab to the front. In `pane` if one is named; otherwise wherever
   * it already is, the focused pane for preference; and failing that as a
   * new tab in the focused pane.
   */
  open(key: number, pane?: number) {
    const named = pane === undefined ? null : this.leaf(pane);
    const leaf =
      named ?? (this.current.tabs.includes(key) ? this.current : this.holding(key)[0]) ?? this.current;
    if (!leaf.tabs.includes(key)) leaf.tabs.push(key);
    this.#activate(leaf, key);
    this.focused = leaf.id;
  }

  /** Take a tab out of one pane. A pane left with none goes with it. */
  closeTab(pane: number, key: number) {
    const leaf = this.leaf(pane);
    if (!leaf) return;
    const at = leaf.tabs.indexOf(key);
    if (at === -1) return;

    leaf.tabs.splice(at, 1);
    if (leaf.last === key) leaf.last = null;
    // The neighbour that slides into its place, else the one before it.
    if (leaf.active === key) leaf.active = leaf.tabs[at] ?? leaf.tabs[at - 1] ?? null;
    if (leaf.tabs.length === 0) this.#removePane(leaf.id);
  }

  /** Take a tab out of every pane: its file was closed, or its shell ended. */
  drop(key: number) {
    for (const leaf of this.holding(key)) this.closeTab(leaf.id, key);
  }

  #removePane(id: number) {
    if (this.leaves.length < 2) return;
    const order = this.leaves.map((leaf) => leaf.id);
    const at = order.indexOf(id);
    this.root = remove(this.root, id);
    if (this.focused === id) this.focused = order[at - 1] ?? order[at + 1];
  }

  /**
   * Put a tab at `index` of a pane's strip, counted as the strip will be
   * once the tab has left where it was; a negative index is the end. Moved
   * to a pane that already has it, the two become the one tab.
   */
  move(key: number, from: number, to: number, index: number) {
    const source = this.leaf(from);
    const target = this.leaf(to);
    if (!source || !target || !source.tabs.includes(key)) return;
    const at = (tabs: number[]) => (index < 0 ? tabs.length : Math.min(index, tabs.length));

    if (from === to) {
      source.tabs.splice(source.tabs.indexOf(key), 1);
      source.tabs.splice(at(source.tabs), 0, key);
      return;
    }
    if (!target.tabs.includes(key)) target.tabs.splice(at(target.tabs), 0, key);
    this.#activate(target, key);
    this.focused = to;
    this.closeTab(from, key);
  }

  /** A new pane beside `pane`, to its right or below it, holding `tabs`. */
  split(dir: SplitDir, tabs: number[], pane = this.focused): number {
    const fresh = this.#leaf(tabs);
    this.root = split(this.root, pane, dir, fresh, this.#ids++);
    this.focused = fresh.id;
    return fresh.id;
  }

  /**
   * Close a pane and keep what was in it: its tabs join the pane beside it.
   * Returns that pane, or null if there was only the one.
   */
  closePane(id: number): number | null {
    const leaf = this.leaf(id);
    if (!leaf || this.leaves.length < 2) return null;
    const order = this.leaves;
    const at = order.findIndex((each) => each.id === id);
    const into = order[at - 1] ?? order[at + 1];

    for (const key of leaf.tabs) if (!into.tabs.includes(key)) into.tabs.push(key);
    if (leaf.active !== null) this.#activate(into, leaf.active);
    const focused = this.focused === id;
    this.root = remove(this.root, id);
    if (focused) this.focused = into.id;
    return into.id;
  }

  /** `:only`: this pane, holding everything the others did. */
  only(id = this.focused) {
    const keep = this.leaf(id);
    if (!keep) return;
    for (const leaf of this.leaves) {
      if (leaf.id === id) continue;
      for (const key of leaf.tabs) if (!keep.tabs.includes(key)) keep.tabs.push(key);
    }
    this.root = keep;
    this.focused = id;
  }

  /** Step the focus to the pane on one side. False if there is none there. */
  focusTowards(towards: Direction): boolean {
    const to = neighbour(this.root, this.focused, towards);
    if (to === null) return false;
    this.focused = to;
    return true;
  }

  /** Step the focus through the panes in order, wrapping at the ends. */
  focusNext(step: 1 | -1) {
    const order = this.leaves.map((leaf) => leaf.id);
    const at = order.indexOf(this.focused);
    this.focused = order[(at + step + order.length) % order.length];
  }

  /** Drag the line after child `index` of a split to `at`, a fraction of the stage. */
  resize(id: number, index: number, at: number) {
    const node = splitOf(this.root, id);
    const box = this.boxes.get(id);
    if (node && box) resize(node, box, index, at);
  }

  /** One empty pane, as a new window has. */
  clear() {
    const fresh = this.#leaf();
    this.root = fresh;
    this.focused = fresh.id;
  }

  /** The layout with its tabs named by `name`, for writing down. */
  save(name: (key: number) => string | null): SavedNode {
    return freeze($state.snapshot(this.root) as Node, name);
  }

  /**
   * Take up a saved layout. `rest` is every tab that should end up somewhere:
   * any the layout does not mention join its first pane. False, having
   * changed nothing, if there is nothing usable in what was saved.
   */
  restore(saved: unknown, key: (name: string) => number | null, rest: number[]): boolean {
    const root = revive(saved, key, () => this.#ids++);
    if (!root) return false;

    const panes = leaves(root);
    const placed = new Set(panes.flatMap((leaf) => leaf.tabs));
    for (const tab of rest) if (!placed.has(tab)) panes[0].tabs.push(tab);
    this.root = root;
    this.focused = panes[0].id;
    return true;
  }
}
