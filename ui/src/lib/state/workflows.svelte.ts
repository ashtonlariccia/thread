/**
 * The workflows of this window: separate desks, one in front at a time.
 *
 * Each has panes and a file tree of its own, and its own terminals (App
 * keeps those, marked with the workflow they belong to). Turning to another
 * puts nothing away: its terminals go on running and its panes are as they
 * were left on coming back. What they share is the files themselves: one
 * open in two workflows is the one buffer, as it is in two panes.
 */
import { Layout } from "./layout.svelte";
import { Tree } from "./tree.svelte";

export type Workflow = {
  id: number;
  /** What it was called by hand; null to go by its first folder. */
  name: string | null;
  layout: Layout;
  tree: Tree;
};

export class Workflows {
  #ids = 1;

  list = $state<Workflow[]>([]);
  #currentId = $state(0);

  /** Whether a tree lists a folder's files above its folders, for each new one. */
  filesFirst = false;
  /** Whether a tree lists a `foo.h` above its `foo.c`, for each new one. */
  headersFirst = false;

  constructor() {
    this.reset();
  }

  #make(name: string | null = null): Workflow {
    const tree = new Tree();
    tree.filesFirst = this.filesFirst;
    tree.headersFirst = this.headersFirst;
    return { id: this.#ids++, name, layout: new Layout(), tree };
  }

  get current(): Workflow {
    return this.list.find((flow) => flow.id === this.#currentId) ?? this.list[0];
  }

  get index(): number {
    return Math.max(0, this.list.findIndex((flow) => flow.id === this.#currentId));
  }

  get layouts(): Layout[] {
    return this.list.map((flow) => flow.layout);
  }

  find(id: number): Workflow | null {
    return this.list.find((flow) => flow.id === id) ?? null;
  }

  /** What a workflow is called: its own name, its first folder's, or its place. */
  label(flow: Workflow): string {
    if (flow.name) return flow.name;
    const root = flow.tree.roots[0];
    if (root) return root.name;
    return `Workflow ${this.list.indexOf(flow) + 1}`;
  }

  /** A new, empty workflow after the others. Not turned to: see `select`. */
  add(name: string | null = null): Workflow {
    this.list.push(this.#make(name));
    return this.list.at(-1)!;
  }

  select(id: number) {
    if (this.find(id)) this.#currentId = id;
  }

  /** Step to the next or previous workflow, wrapping at the ends. */
  step(by: 1 | -1) {
    const count = this.list.length;
    if (count > 1) this.#currentId = this.list[(this.index + by + count) % count].id;
  }

  rename(id: number, name: string) {
    const flow = this.find(id);
    if (flow) flow.name = name.trim() || null;
  }

  /** Take a workflow away; the one before it comes to the front. Never the last. */
  remove(id: number) {
    if (this.list.length < 2) return;
    const at = this.list.findIndex((flow) => flow.id === id);
    if (at === -1) return;
    this.list.splice(at, 1);
    if (this.#currentId === id) this.#currentId = this.list[Math.max(0, at - 1)].id;
  }

  /** One empty workflow, as a new window has. */
  reset() {
    const fresh = this.#make();
    this.list = [fresh];
    this.#currentId = fresh.id;
  }
}
