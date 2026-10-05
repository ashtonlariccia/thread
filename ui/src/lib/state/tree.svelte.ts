/**
 * The file tree: the folder open in this window, and what is unfolded in it.
 *
 * Folders are listed one at a time, when they are unfolded — opening a project
 * never walks the whole of it. What the listing leaves out (dotfiles, unless
 * the settings file says otherwise) is decided by the backend.
 */
import { invoke } from "@tauri-apps/api/core";
import { message, open } from "@tauri-apps/plugin-dialog";

import { baseName, segmentsBelow } from "../paths";

export type Entry = { name: string; path: string; dir: boolean };

/** One visible row, in the order the tree draws them. */
export type TreeRow = {
  entry: Entry;
  /** The root is 0, its children 1, and so on. */
  depth: number;
  /** For a folder, whether it is unfolded. */
  open: boolean;
  root: boolean;
};

export class Tree {
  /** The folder the tree was opened at, or null when there is none. */
  root = $state<Entry | null>(null);

  /** Each listed folder's children, by path. */
  #children = $state<Record<string, Entry[]>>({});
  /** The folders that are unfolded, by path. */
  #open = $state<Record<string, boolean>>({});
  /** When each listed folder's contents last changed, as of its listing. */
  #stamps = new Map<string, number | null>();
  #polling = false;

  /** Every row the unfolded tree shows, root first. */
  get rows(): TreeRow[] {
    if (!this.root) return [];

    const rows: TreeRow[] = [];
    const walk = (entry: Entry, depth: number) => {
      const open = entry.dir && this.#open[entry.path] === true;
      rows.push({ entry, depth, open, root: depth === 0 });
      if (open) for (const child of this.#children[entry.path] ?? []) walk(child, depth + 1);
    };
    walk(this.root, 0);
    return rows;
  }

  /** What is directly inside the root: what the collapsed rail shows. */
  get topLevel(): Entry[] {
    return this.root ? (this.#children[this.root.path] ?? []) : [];
  }

  isOpen(path: string): boolean {
    return this.#open[path] === true;
  }

  /** File → Open Folder. */
  async openDialog() {
    const picked = await open({ title: "Open Folder", directory: true });
    if (typeof picked === "string") await this.open(picked);
  }

  /** Make `path` the tree's root, replacing whatever folder was open. */
  async open(path: string) {
    // `C:\src\thread\` and `C:\src\thread` are the same folder, and only one
    // of them has a last component to name it by.
    const trimmed = path.length > 3 ? path.replace(/[\\/]+$/, "") : path;

    this.#children = {};
    this.#open = { [trimmed]: true };
    this.#stamps.clear();
    this.root = { name: baseName(trimmed) || trimmed, path: trimmed, dir: true };

    const error = await this.#load(trimmed);
    if (error !== null) {
      this.close();
      void message(error, { title: "Thread", kind: "error" });
    }
  }

  close() {
    this.root = null;
    this.#children = {};
    this.#open = {};
    this.#stamps.clear();
  }

  /** Fold or unfold a folder. */
  async toggle(path: string) {
    if (this.#open[path]) this.#open[path] = false;
    else await this.expand(path);
  }

  /** Unfold a folder, listing it afresh: it was not being watched while folded. */
  async expand(path: string) {
    this.#open[path] = true;
    if ((await this.#load(path)) !== null) this.#forget(path);
  }

  /**
   * Unfold whatever folders lie between the root and `path`, so its row is
   * on screen. Does nothing for a file outside the open folder, or one the
   * tree leaves out.
   */
  async reveal(path: string) {
    if (!this.root) return;
    const names = segmentsBelow(this.root.path, path);
    if (!names) return;

    let dir = this.root.path;
    // Every name but the last is a folder to unfold; the last is the file.
    for (const name of names.slice(0, -1)) {
      if (!this.#open[dir] || !(dir in this.#children)) await this.expand(dir);
      // Walked by the listing rather than by joining strings, so the paths
      // are spelled exactly as the tree's own rows spell them.
      const next = this.#children[dir]?.find(
        (e) => e.dir && e.name.toLowerCase() === name.toLowerCase(),
      );
      if (!next) return;
      dir = next.path;
    }
    if (!this.#open[dir] || !(dir in this.#children)) await this.expand(dir);
  }

  /**
   * Re-list every unfolded folder: the exclude list changed, so what each one
   * shows may have too.
   */
  async refresh() {
    for (const dir of this.#watched()) await this.#load(dir);
  }

  /**
   * Re-list any unfolded folder whose contents have changed on disk.
   *
   * Called on the same timer that checks open files. One `stat` per unfolded
   * folder; a folder is only read again once something in it has been added,
   * removed or renamed.
   */
  async poll() {
    if (this.#polling || !this.root) return;
    this.#polling = true;
    try {
      const dirs = this.#watched();
      const stamps = await invoke<(number | null)[]>("dir_stamps", { paths: dirs });
      for (const [index, dir] of dirs.entries()) {
        if (stamps[index] === this.#stamps.get(dir)) continue;
        if ((await this.#load(dir)) !== null && dir !== this.root?.path) this.#forget(dir);
      }
    } catch (e) {
      console.error("checking folders on disk failed", e);
    } finally {
      this.#polling = false;
    }
  }

  /** The folders whose listings are on screen: unfolded, and listed. */
  #watched(): string[] {
    return Object.keys(this.#children).filter((dir) => this.#open[dir]);
  }

  /** List one folder. Returns the error message, or null if it worked. */
  async #load(dir: string): Promise<string | null> {
    try {
      // Stamp first: a change landing between the two is then seen as newer
      // than the listing and picked up by the next poll, rather than missed.
      const [stamp] = await invoke<(number | null)[]>("dir_stamps", { paths: [dir] });
      const entries = await invoke<Entry[]>("read_dir", { path: dir });
      this.#children[dir] = entries;
      this.#stamps.set(dir, stamp);
      return null;
    } catch (e) {
      return String(e);
    }
  }

  /** A folder that can no longer be listed: gone, most likely. Fold it away. */
  #forget(dir: string) {
    delete this.#children[dir];
    delete this.#open[dir];
    this.#stamps.delete(dir);
  }
}
