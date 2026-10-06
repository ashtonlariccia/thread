/**
 * The files open in this window.
 *
 * The text itself lives in the editor (`editor.ts`); this holds what the rest
 * of the window needs to know about each file — where it is, whether it has
 * unsaved changes — and is the one place files are opened, saved and closed.
 *
 * It also keeps each file in step with the disk. Other programs write to open
 * files all the time (git, a formatter, an agent in a terminal), and an editor
 * that goes on showing the old text will overwrite their work on the next save.
 */
import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";

import { EditorHost, type Cursor } from "../editor";
import { detectIndent, resolveIndent, type Detected, type Indent } from "../indent";
import { languageOf } from "../languages";
import { baseName, samePath, segmentsBelow } from "../paths";
import { pickFiles, pickSave } from "../pick";
import { loadSyntax, setCustomSyntax, syntaxRevision } from "../syntax";
import { syntaxTheme } from "../themes";
import { loadVim, type VimApi, type VimHooks, type VimMode } from "../vim";
import { substitutePreview } from "../vimSubstitute";
import { DEFAULTS, type Config } from "./config.svelte";
import type { Layout } from "./layout.svelte";

export type Eol = "lf" | "crlf";

/** What a file looked like on disk when it was last read or written. */
export type Stamp = { modified: number; len: number };

export type Doc = {
  key: number;
  /** Null for a new file that has never been saved. */
  path: string | null;
  name: string;
  /** How the file was found on disk; restored on save. */
  eol: Eol;
  bom: boolean;
  dirty: boolean;
  /** Null when there is no file on disk: never saved, or since deleted. */
  stamp: Stamp | null;
  /** How the text was indented when it was opened; null if it gave no clue. */
  detected: Detected | null;
  /** What Tab does in this file: the config, the language, and `detected`. */
  indent: Indent;
};

/** What `read_file` returns. */
type Loaded = {
  path: string;
  name: string;
  text: string;
  eol: Eol;
  bom: boolean;
  stamp: Stamp | null;
};

export type UnsavedChoice = "save" | "discard" | "cancel";

/** Told about a file's life, for whatever follows files around: language servers. */
export type DocWatcher = {
  opened: (doc: Doc) => void;
  /** Its text changed. */
  changed: (key: number) => void;
  saved: (doc: Doc) => void;
  /** It has a different path, or name, than it had: `from` is the old path. */
  moved: (doc: Doc, from: string | null) => void;
  closed: (doc: Doc) => void;
};

function report(error: unknown) {
  void message(String(error), { title: "Thread", kind: "error" });
}

function sameStamp(a: Stamp | null, b: Stamp | null): boolean {
  if (!a || !b) return a === b;
  return a.modified === b.modified && a.len === b.len;
}

export class Documents {
  list = $state<Doc[]>([]);
  /** Where the cursor is in each pane's editor, by pane. */
  cursors = $state<Record<number, Cursor>>({});

  /** The vim mode each pane's editor is in, by pane. */
  vimModes = $state<Record<number, VimMode>>({});
  /** Whether vim motions are on. */
  vimOn = $state(false);
  /**
   * Where each pane shows vim's `:` line and messages, by pane; set by
   * whoever draws it.
   */
  vimLines: Record<number, HTMLElement | null> = {};
  /**
   * What vim's commands for files and panes do; set by whoever owns the
   * panes. Undo and redo are not theirs to say: those are the editor's.
   */
  vimHooks: Omit<VimHooks, "undo" | "redo"> | null = null;
  /** Whoever wants to hear what happens to the files. */
  watcher: DocWatcher | null = null;

  /** Set while the user is being asked what to do with unsaved files. */
  asking = $state.raw<{ docs: Doc[]; resolve: (proceed: boolean) => void } | null>(null);

  /** Set while the user is being asked about a file that changed under their edits. */
  changed = $state.raw<{ doc: Doc; resolve: (reload: boolean) => void } | null>(null);

  readonly editor = new EditorHost({
    ondirty: (key, dirty) => {
      const doc = this.find(key);
      if (doc && doc.dirty !== dirty) doc.dirty = dirty;
    },
    onchange: (key) => this.watcher?.changed(key),
    oncursor: (pane, cursor) => (this.cursors[pane] = cursor),
    onview: (pane, view) =>
      this.#vim?.watch(
        view,
        (mode) => (this.vimModes[pane] = mode),
        () => this.vimLines[pane] ?? null,
      ),
    onfocus: (pane) => this.panes.focus(pane),
  });

  constructor(private readonly panes: Layout) {}

  #config: Config = DEFAULTS;
  /** The vim extension, once fetched and while switched on. */
  #vim: VimApi | null = null;
  #vimWanted = false;
  #nextKey = 1;
  #nextUntitled = 1;
  /** The language each file's grammar was last asked for, so it is asked once. */
  #grammars = new Map<number, string>();
  /** Files being written right now, whose stamps are about to move on purpose. */
  #saving = new Set<number>();
  #checking = false;

  /** The file showing in the focused pane, if a file is what is showing there. */
  get activeKey(): number | null {
    return this.panes.activeFile;
  }

  get active(): Doc | null {
    return this.find(this.activeKey);
  }

  /** Where the cursor is in the focused pane. */
  get cursor(): Cursor {
    return this.cursors[this.panes.focused] ?? { line: 1, col: 1 };
  }

  /** The vim mode the focused pane is in, or null while vim motions are off. */
  get vimMode(): VimMode | null {
    return this.vimOn ? (this.vimModes[this.panes.focused] ?? "normal") : null;
  }

  get dirty(): Doc[] {
    return this.list.filter((d) => d.dirty);
  }

  /** A question is on screen; the window behind it is not taking commands. */
  get busy(): boolean {
    return this.asking !== null || this.changed !== null;
  }

  find(key: number | null): Doc | null {
    return this.list.find((d) => d.key === key) ?? null;
  }

  /**
   * Bring a file to the front: in `pane` if one is named, and otherwise
   * wherever it already is, or as a new tab of the focused pane.
   */
  select(key: number, pane?: number) {
    this.panes.open(key, pane);
  }

  /** Take on a new config: every open file is re-dressed, not just the next. */
  configure(config: Config) {
    this.#config = config;
    this.editor.setLook({
      fontFamily: config.editor.font_family,
      fontSize: config.editor.font_size,
      lineHeight: config.editor.line_height,
      lineNumbers: config.editor.line_numbers,
      relativeLineNumbers: config.editor.relative_line_numbers,
      wordWrap: config.editor.word_wrap,
      smoothCaret: config.editor.smooth_caret,
      autoClose: config.editor.auto_close,
    });
    this.editor.setHighlightStyle(syntaxTheme(config.theme.syntax));
    this.#setVim(config.vim.enabled);
    // Before the files are dressed: what language each is can depend on it.
    setCustomSyntax(config.syntax);
    for (const doc of this.list) this.#dress(doc);
  }

  #setVim(enabled: boolean) {
    if (enabled === this.#vimWanted) return;
    this.#vimWanted = enabled;

    if (!enabled) {
      this.#vim = null;
      this.vimOn = false;
      this.editor.setVim([]);
      return;
    }

    // Looked up as each command runs, so whoever sets them can do so late.
    void loadVim({
      write: () => this.vimHooks?.write(),
      quit: (force) => this.vimHooks?.quit(force),
      writeQuit: () => this.vimHooks?.writeQuit(),
      cycle: (step) => this.vimHooks?.cycle(step),
      split: (dir, file) => this.vimHooks?.split(dir, file),
      fresh: (dir) => this.vimHooks?.fresh(dir),
      edit: (file) => this.vimHooks?.edit(file),
      close: () => this.vimHooks?.close(),
      only: () => this.vimHooks?.only(),
      wincmd: (arg) => this.vimHooks?.wincmd(arg),
      quitAll: (force) => this.vimHooks?.quitAll(force),
      writeAll: () => this.vimHooks?.writeAll(),
      writeQuitAll: () => this.vimHooks?.writeQuitAll(),
      undo: (view) => void this.editor.travelIn(view, "undo"),
      redo: (view) => void this.editor.travelIn(view, "redo"),
    })
      .then((vim) => {
        // Switched off again while it was being fetched.
        if (!this.#vimWanted) return;
        this.#vim = vim;
        this.vimOn = true;
        this.editor.setVim([vim.extension, substitutePreview()]);
      })
      .catch((e) => {
        this.#vimWanted = false;
        console.error("loading vim motions failed", e);
      });
  }

  /**
   * The indentation a file gets: `[editor]`, then its language's override,
   * then — if detection is on — whatever the file itself was found using.
   */
  #indentFor(name: string, detected: Detected | null): Indent {
    const override = this.#config.language[languageOf(name).toLowerCase()] ?? {};
    return resolveIndent({ ...this.#config.editor, ...override }, detected);
  }

  /** Apply everything about a file that follows from its name and the config. */
  #dress(doc: Doc) {
    doc.indent = this.#indentFor(doc.name, doc.detected);
    this.editor.setIndent(doc.key, doc.indent);

    const language = languageOf(doc.name);
    // With what it is built from, for a language of the user's own: the same
    // name with a different description is a different grammar.
    const wanted = language + syntaxRevision(language);
    if (this.#grammars.get(doc.key) === wanted) return;
    this.#grammars.set(doc.key, wanted);

    const key = doc.key;
    // The grammar is fetched on first use, so the file shows as plain text
    // for the moment that takes and is coloured when it lands.
    void loadSyntax(language).then((grammar) => {
      // Closed, or renamed into another language, while it was loading.
      if (this.#grammars.get(key) !== wanted) return;

      this.editor.setLanguage(key, grammar ?? []);
    });
  }

  /**
   * File → New File: an empty buffer with nowhere to live until it is saved.
   * Not brought to the front if `show` is false; whoever asked puts it somewhere.
   */
  newFile({ show = true } = {}): number {
    const key = this.#nextKey++;
    const name = `Untitled-${this.#nextUntitled++}`;
    const indent = this.#indentFor(name, null);
    this.editor.create(key, "", indent);
    this.list.push({
      key,
      path: null,
      name,
      eol: "lf",
      bom: false,
      dirty: false,
      stamp: null,
      detected: null,
      indent,
    });
    this.#dress(this.list.at(-1)!);
    this.watcher?.opened(this.list.at(-1)!);
    if (show) this.select(key);
    return key;
  }

  /** File → Open File. */
  async openDialog() {
    for (const path of (await pickFiles()) ?? []) await this.open(path);
  }

  /**
   * Open a file in the editor, or go to it if it is already open.
   *
   * `quiet` is for files nobody just asked for by hand — the ones a session
   * is restoring — where a file that has since gone is skipped, not announced.
   * With `show` false it is opened and not brought to the front: whoever
   * asked puts it in a pane. Returns the file's key, or null if it would not open.
   */
  async open(path: string, { quiet = false, show = true } = {}): Promise<number | null> {
    // Opening a file that is already open goes to it, rather than making a
    // second buffer whose edits would fight the first's on save.
    const existing = this.list.find((d) => d.path !== null && samePath(d.path, path));
    if (existing) {
      if (show) this.select(existing.key);
      return existing.key;
    }

    try {
      const loaded = await invoke<Loaded>("read_file", { path });
      const key = this.#nextKey++;
      const detected = detectIndent(loaded.text);
      const indent = this.#indentFor(loaded.name, detected);
      this.editor.create(key, loaded.text, indent);
      this.list.push({
        key,
        path: loaded.path,
        name: loaded.name,
        eol: loaded.eol,
        bom: loaded.bom,
        dirty: false,
        stamp: loaded.stamp,
        detected,
        indent,
      });
      this.#dress(this.list.at(-1)!);
      this.watcher?.opened(this.list.at(-1)!);
      if (show) this.select(key);
      return key;
    } catch (e) {
      if (!quiet) report(e);
      return null;
    }
  }

  /**
   * Write a file where it already lives; a file that lives nowhere yet is
   * asked where to go. Returns whether it was saved.
   */
  async save(key: number | null = this.activeKey): Promise<boolean> {
    let doc = this.find(key);
    if (!doc) return false;
    if (doc.path === null) return this.saveAs(doc.key);

    // The poll may be up to a second behind. Catch a change that landed since
    // *before* writing over it, so it is asked about rather than destroyed.
    await this.checkDisk();
    doc = this.find(key);
    if (!doc || doc.path === null) return false;

    return this.#write(doc, doc.path);
  }

  /** Write a file somewhere new, and carry on editing it there. */
  async saveAs(key: number | null = this.activeKey): Promise<boolean> {
    const doc = this.find(key);
    if (!doc) return false;

    const picked = await pickSave(doc.path ?? doc.name);
    return picked ? this.#write(doc, picked) : false;
  }

  async #write(doc: Doc, path: string): Promise<boolean> {
    this.#saving.add(doc.key);
    try {
      doc.stamp = await invoke<Stamp | null>("write_file", {
        path,
        text: this.editor.text(doc.key),
        eol: doc.eol,
        bom: doc.bom,
      });
    } catch (e) {
      report(e);
      return false;
    } finally {
      this.#saving.delete(doc.key);
    }

    const from = doc.path;
    doc.path = path;
    doc.name = baseName(path);
    // A new name can mean a new language, with settings of its own.
    this.#dress(doc);
    this.editor.markSaved(doc.key);
    if (from === null || !samePath(from, path)) this.watcher?.moved(doc, from);
    this.watcher?.saved(doc);
    return true;
  }

  /**
   * A file or folder was renamed on disk from inside Thread. Open files that
   * were at the old path, or under it, carry on at the new one — same buffer,
   * same unsaved edits, same undo history.
   */
  renamed(from: string, to: string) {
    const sep = to.includes("\\") ? "\\" : "/";
    for (const doc of this.list) {
      if (doc.path === null) continue;

      const below = segmentsBelow(from, doc.path);
      const next = samePath(doc.path, from) ? to : below ? [to, ...below].join(sep) : null;
      if (next === null) continue;

      const was = doc.path;
      doc.path = next;
      doc.name = baseName(next);
      // A new name can mean a new language, with settings of its own.
      this.#dress(doc);
      this.watcher?.moved(doc, was);
    }
  }

  /**
   * Close a file, in every pane that has it, asking first if it has unsaved
   * changes. `force` does not ask: it is `:q!`, which is the answer already.
   */
  async close(key: number | null = this.activeKey, { force = false } = {}) {
    const doc = this.find(key);
    if (!doc) return;
    if (doc.dirty && !force && !(await this.confirm([doc]))) return;
    this.#remove(doc);
  }

  /**
   * Close every open file for which `under` is true, asking once about all
   * the unsaved ones among them. Returns false, having closed nothing, if
   * that question was cancelled.
   */
  async closeWhere(under: (path: string) => boolean): Promise<boolean> {
    const docs = this.list.filter((doc) => doc.path !== null && under(doc.path));
    if (!(await this.confirm(docs.filter((doc) => doc.dirty)))) return false;
    for (const doc of docs) this.#remove(doc);
    return true;
  }

  /**
   * Save every file with unsaved changes, one after another. False as soon
   * as one does not save, which leaves the rest as they were.
   */
  async saveAll(): Promise<boolean> {
    for (const doc of this.dirty) if (!(await this.save(doc.key))) return false;
    return true;
  }

  /**
   * Close every file, asking once about all the unsaved ones. `force` does
   * not ask. False, having closed nothing, if the question was cancelled.
   */
  async closeAll({ force = false } = {}): Promise<boolean> {
    if (!force && !(await this.confirm())) return false;
    this.clear();
    return true;
  }

  /**
   * Close every file, unsaved or not, without a word. For when the window
   * moves to another machine: whoever calls this has already asked.
   */
  clear() {
    for (const doc of [...this.list]) this.#remove(doc);
  }

  #remove(doc: Doc) {
    const index = this.list.findIndex((d) => d.key === doc.key);
    if (index === -1) return;
    this.list.splice(index, 1);
    // Whatever vim was showing was about this file, or was asked from it.
    for (const leaf of this.panes.holding(doc.key)) {
      if (leaf.active === doc.key) this.vimLines[leaf.id]?.replaceChildren();
    }
    this.watcher?.closed(doc);
    this.editor.drop(doc.key);
    this.#grammars.delete(doc.key);
    // Each pane that had it shows the neighbour that slides into its place.
    this.panes.drop(doc.key);
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

  // --- keeping up with the disk ---------------------------------------------

  /**
   * Compare every open file with what is on disk, and catch up where they differ.
   *
   * Called on a timer and whenever the window regains focus. It costs one
   * `stat` per open file; a file is only read once its stamp has moved.
   */
  async checkDisk() {
    if (this.#checking) return;
    this.#checking = true;
    try {
      const docs = this.list.filter((d) => d.path !== null && !this.#saving.has(d.key));
      if (docs.length === 0) return;

      const stamps = await invoke<(Stamp | null)[]>("file_stamps", {
        paths: docs.map((d) => d.path),
      });
      for (const [index, doc] of docs.entries()) await this.#sync(doc, stamps[index]);
    } catch (e) {
      console.error("checking files on disk failed", e);
    } finally {
      this.#checking = false;
    }
  }

  /** Bring one file into line with the disk, given the stamp it has there now. */
  async #sync(doc: Doc, onDisk: Stamp | null) {
    // Closed, saved elsewhere, or mid-save since the stamps were taken.
    if (this.find(doc.key) !== doc || doc.path === null || this.#saving.has(doc.key)) return;
    if (sameStamp(doc.stamp, onDisk)) return;

    if (!onDisk) {
      // Deleted or moved away. The buffer is now the only copy, which is
      // exactly what "unsaved" means — so closing it asks, and saving puts
      // the file back.
      doc.stamp = null;
      this.editor.markUnsaved(doc.key);
      return;
    }

    let loaded: Loaded;
    try {
      loaded = await invoke<Loaded>("read_file", { path: doc.path });
    } catch {
      // Caught mid-write, or no longer text. Remember this stamp so the same
      // unreadable version is not retried every tick; the write finishing
      // moves the stamp again and brings us back.
      doc.stamp = onDisk;
      return;
    }
    if (this.find(doc.key) !== doc) return;
    doc.stamp = loaded.stamp;

    if (loaded.text === this.editor.text(doc.key)) {
      // Rewritten with what is already here — a `touch`, or a tool that put
      // back the same bytes. Nothing to show, and nothing unsaved either.
      doc.eol = loaded.eol;
      doc.bom = loaded.bom;
      this.editor.markSaved(doc.key);
      return;
    }

    // Unedited files simply follow the disk. Edited ones are the user's call:
    // either side winning silently loses someone's work.
    if (doc.dirty && !(await this.#askReload(doc))) return;
    if (this.find(doc.key) !== doc) return;

    this.editor.replace(doc.key, loaded.text);
    doc.eol = loaded.eol;
    doc.bom = loaded.bom;
    this.editor.markSaved(doc.key);
  }

  #askReload(doc: Doc): Promise<boolean> {
    return new Promise((resolve) => (this.changed = { doc, resolve }));
  }

  /** The answer to "this file changed on disk": reload it, or keep the edits. */
  answerChanged(reload: boolean) {
    const changed = this.changed;
    if (!changed) return;
    this.changed = null;
    changed.resolve(reload);
  }
}
