/**
 * The files open in this window.
 *
 * The text itself lives in the editor (`editor.ts`); this holds what the rest
 * of the window needs to know about each file — where it is, whether it has
 * unsaved changes — and is the one place files are opened, saved and closed.
 */
import { invoke } from "@tauri-apps/api/core";
import { message, open, save } from "@tauri-apps/plugin-dialog";

import { EditorHost, type Cursor } from "../editor";
import { baseName, samePath } from "../paths";

export type Eol = "lf" | "crlf";

export type Doc = {
  key: number;
  path: string;
  name: string;
  /** How the file was found on disk; restored on save. */
  eol: Eol;
  bom: boolean;
  dirty: boolean;
};

/** What `read_file` returns. */
type Loaded = { path: string; name: string; text: string; eol: Eol; bom: boolean };

export type UnsavedChoice = "save" | "discard" | "cancel";

function report(error: unknown) {
  void message(String(error), { title: "Thread", kind: "error" });
}

export class Documents {
  list = $state<Doc[]>([]);
  activeKey = $state<number | null>(null);
  cursor = $state<Cursor>({ line: 1, col: 1 });

  /** Set while the user is being asked what to do with unsaved files. */
  asking = $state.raw<{ docs: Doc[]; resolve: (proceed: boolean) => void } | null>(null);

  readonly editor = new EditorHost({
    ondirty: (key, dirty) => {
      const doc = this.find(key);
      if (doc && doc.dirty !== dirty) doc.dirty = dirty;
    },
    oncursor: (cursor) => (this.cursor = cursor),
  });

  #nextKey = 1;

  get active(): Doc | null {
    return this.find(this.activeKey);
  }

  get dirty(): Doc[] {
    return this.list.filter((d) => d.dirty);
  }

  find(key: number | null): Doc | null {
    return this.list.find((d) => d.key === key) ?? null;
  }

  select(key: number | null) {
    this.activeKey = key;
    this.editor.show(key);
  }

  /** File → Open File. */
  async openDialog() {
    const picked = await open({ title: "Open File", multiple: true });
    if (!picked) return;
    for (const path of Array.isArray(picked) ? picked : [picked]) await this.open(path);
  }

  async open(path: string) {
    // Opening a file that is already open goes to it, rather than making a
    // second buffer whose edits would fight the first's on save.
    const existing = this.list.find((d) => samePath(d.path, path));
    if (existing) {
      this.select(existing.key);
      return;
    }

    try {
      const loaded = await invoke<Loaded>("read_file", { path });
      const key = this.#nextKey++;
      this.editor.create(key, loaded.text);
      this.list.push({
        key,
        path: loaded.path,
        name: loaded.name,
        eol: loaded.eol,
        bom: loaded.bom,
        dirty: false,
      });
      this.select(key);
    } catch (e) {
      report(e);
    }
  }

  /** Write a file where it already lives. Returns whether it was saved. */
  async save(key: number | null = this.activeKey): Promise<boolean> {
    const doc = this.find(key);
    return doc ? this.#write(doc, doc.path) : false;
  }

  /** Write a file somewhere new, and carry on editing it there. */
  async saveAs(key: number | null = this.activeKey): Promise<boolean> {
    const doc = this.find(key);
    if (!doc) return false;

    const picked = await save({ title: "Save As", defaultPath: doc.path });
    return picked ? this.#write(doc, picked) : false;
  }

  async #write(doc: Doc, path: string): Promise<boolean> {
    try {
      await invoke("write_file", {
        path,
        text: this.editor.text(doc.key),
        eol: doc.eol,
        bom: doc.bom,
      });
    } catch (e) {
      report(e);
      return false;
    }

    doc.path = path;
    doc.name = baseName(path);
    this.editor.markSaved(doc.key);
    return true;
  }

  /** Close a file, asking first if it has unsaved changes. */
  async close(key: number | null = this.activeKey) {
    const doc = this.find(key);
    if (!doc) return;
    if (doc.dirty && !(await this.confirm([doc]))) return;

    const index = this.list.findIndex((d) => d.key === doc.key);
    this.list.splice(index, 1);
    this.editor.drop(doc.key);
    // The neighbour that slid into its place, else the one before it.
    if (this.activeKey === doc.key) this.select((this.list[index] ?? this.list.at(-1))?.key ?? null);
  }

  /**
   * Ask what to do with unsaved files before something would lose them.
   *
   * Resolves true when it is safe to go ahead: every file was saved, or the
   * user chose to discard. False means stop — they cancelled, or a save failed.
   */
  confirm(docs: Doc[] = this.dirty): Promise<boolean> {
    if (docs.length === 0) return Promise.resolve(true);
    // One question at a time; a second request while one is up is a "no".
    if (this.asking) return Promise.resolve(false);
    return new Promise((resolve) => (this.asking = { docs, resolve }));
  }

  async answer(choice: UnsavedChoice) {
    const asking = this.asking;
    if (!asking) return;
    this.asking = null;

    if (choice === "save") {
      for (const doc of asking.docs) {
        if (!(await this.save(doc.key))) {
          asking.resolve(false);
          return;
        }
      }
    }
    asking.resolve(choice !== "cancel");
  }
}
