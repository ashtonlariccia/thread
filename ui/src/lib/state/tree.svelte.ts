/**
 * The file tree: the folders open in this window, and what is unfolded in them.
 *
 * Any number of folders can be open at once, each a root of its own. Opening
 * another adds to them; a folder only goes away when it is closed.
 *
 * Folders are listed one at a time, when they are unfolded — opening a project
 * never walks the whole of it. What the listing leaves out (dotfiles, unless
 * the settings file says otherwise) is decided by the backend.
 */
import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";

import { baseName, samePath, segmentsBelow } from "../paths";
import { pickFolder } from "../pick";

export type Entry = { name: string; path: string; dir: boolean };

/** One visible row, in the order the tree draws them. */
export type TreeRow = {
  /**
   * Unique among the rows. The path alone is not: with `C:\a` and `C:\a\b`
   * both open as roots, `C:\a\b` is on screen twice.
   */
  id: string;
  entry: Entry;
  /** A root is 0, its children 1, and so on. */
  depth: number;
  /** For a folder, whether it is unfolded. */
  open: boolean;
  root: boolean;
};

export class Tree {
  /** The folders that have been opened, in the order they were. */
  roots = $state<Entry[]>([]);

  /** Each listed folder's children, by path. */
  #children = $state<Record<string, Entry[]>>({});
  /** The folders that are unfolded, by path. */
  #open = $state<Record<string, boolean>>({});
  /** When each listed folder's contents last changed, as of its listing. */
  #stamps = new Map<string, number | null>();
  #polling = false;

  /** Every row the tree shows: each root, and whatever is unfolded under it. */
  get rows(): TreeRow[] {
    const rows: TreeRow[] = [];
    for (const root of this.roots) {
      const walk = (entry: Entry, depth: number) => {
        const open = entry.dir && this.#open[entry.path] === true;
        rows.push({
          id: `${root.path}\n${entry.path}`,
          entry,
          depth,
          open,
          root: depth === 0,
        });
        if (open) for (const child of this.#children[entry.path] ?? []) walk(child, depth + 1);
      };
      walk(root, 0);
    }
    return rows;
  }

  /** File → Open Folder. */
  async openDialog() {
    const picked = await pickFolder();
    if (picked !== null) await this.open(picked);
  }

  /** Every unfolded folder, the opened ones included. */
  get unfolded(): string[] {
    return Object.keys(this.#open).filter((dir) => this.#open[dir]);
  }

  /**
   * Put back the folders of an earlier session, folded and unfolded as they
   * were. One that has since gone is left out without comment.
   */
  async restore(folders: string[], unfolded: string[]) {
    for (const folder of folders) await this.open(folder, { quiet: true });

    const wanted = new Set(unfolded);
    for (const root of this.roots) if (!wanted.has(root.path)) this.#open[root.path] = false;
    // Shortest first, so a folder is listed before the folders inside it.
    for (const dir of [...unfolded].sort((a, b) => a.length - b.length)) {
      const inside = this.roots.some((root) => segmentsBelow(root.path, dir) !== null);
      if (inside) await this.expand(dir);
    }
  }

  /**
   * Add `path` to the open folders, below the ones already there. A folder
   * that is already open is unfolded rather than added twice.
   */
  async open(path: string, { quiet = false } = {}) {
    // `C:\src\thread\` and `C:\src\thread` are the same folder, and only one
    // of them has a last component to name it by.
    const trimmed = path.length > 3 ? path.replace(/[\\/]+$/, "") : path;

    const existing = this.roots.find((root) => samePath(root.path, trimmed));
    if (existing) {
      await this.expand(existing.path);
      return;
    }

    const error = await this.#load(trimmed);
    if (error !== null) {
      this.#forget(trimmed);
      if (!quiet) void message(error, { title: "Thread", kind: "error" });
      return;
    }
    this.#open[trimmed] = true;
    this.roots.push({ name: baseName(trimmed) || trimmed, path: trimmed, dir: true });
  }

  /** Close one open folder. What is unfolded under the others is untouched. */
  close(path: string) {
    this.roots = this.roots.filter((root) => root.path !== path);
    this.#prune();
  }

  /** Close every open folder. */
  clear() {
    this.roots = [];
    this.#prune();
  }

  /** Fold or unfold a folder. */
  async toggle(path: string) {
    if (this.#open[path]) this.#open[path] = false;
    else await this.expand(path);
  }

  /** Fold a folder away, and what is unfolded inside it with it. An opened folder stays. */
  fold(path: string) {
    for (const open of Object.keys(this.#open)) {
      const inside = samePath(open, path) || segmentsBelow(path, open) !== null;
      if (inside && !this.#isRoot(open)) this.#open[open] = false;
    }
  }

  /** Unfold a folder, listing it afresh: it was not being watched while folded. */
  async expand(path: string) {
    this.#open[path] = true;
    if ((await this.#load(path)) !== null && !this.#isRoot(path)) this.#forget(path);
  }

  /**
   * Unfold whatever folders lie between a root and `path`, so its row is on
   * screen. Does nothing for a file outside every open folder, or one the tree
   * leaves out.
   */
  async reveal(path: string) {
    // The deepest root that contains it: with a project and one of its own
    // subfolders both open, the file belongs to the more specific one.
    const root = this.roots
      .filter((r) => segmentsBelow(r.path, path) !== null)
      .sort((a, b) => b.path.length - a.path.length)[0];
    const names = root && segmentsBelow(root.path, path);
    if (!names) return;

    let dir = root.path;
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

  /** Re-list one folder now, rather than at the next poll: it was just changed. */
  async reload(dir: string) {
    if (dir in this.#children) await this.#load(dir);
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
    if (this.#polling || this.roots.length === 0) return;
    this.#polling = true;
    try {
      const dirs = this.#watched();
      const stamps = await invoke<(number | null)[]>("dir_stamps", { paths: dirs });
      for (const [index, dir] of dirs.entries()) {
        if (stamps[index] === this.#stamps.get(dir)) continue;
        // A root that has gone keeps its row, so it can be seen and closed.
        if ((await this.#load(dir)) !== null && !this.#isRoot(dir)) this.#forget(dir);
      }
    } catch (e) {
      console.error("checking folders on disk failed", e);
    } finally {
      this.#polling = false;
    }
  }

  #isRoot(path: string): boolean {
    return this.roots.some((root) => root.path === path);
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

  /**
   * Drop what is known about folders no longer under any root, so a closed
   * project is not still being polled.
   */
  #prune() {
    const kept = (dir: string) =>
      this.roots.some((root) => root.path === dir || segmentsBelow(root.path, dir) !== null);

    for (const dir of new Set([...Object.keys(this.#children), ...Object.keys(this.#open)])) {
      if (!kept(dir)) this.#forget(dir);
    }
  }
}
