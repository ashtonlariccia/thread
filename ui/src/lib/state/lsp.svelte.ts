/**
 * The language servers of this window: which there are, which are switched
 * on, and which open file each one is looking after.
 *
 * A server is started the first time a file in one of its languages is open
 * while it is switched on, once for each folder such files are in, and from
 * then on is told what those files say as they are edited. What it sends
 * back is put into the editor: completions as they are asked for, and its
 * complaints as it makes them.
 *
 * A server runs where the files are: on this machine, or for a window
 * working on a remote, on the remote. Which are installed is asked of
 * whichever of the two the window is on.
 */
import { invoke } from "@tauri-apps/api/core";
import { autocompletion, type CompletionSource } from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

import { diagnostics, setDiagnostics } from "../diagnostics";
import { languageOf } from "../languages";
import {
  LspClient,
  toCompletions,
  toDiagnostic,
  toPosition,
  toUri,
  uriKey,
  type CompletionItem,
} from "../lsp";
import { dirName, segmentsBelow } from "../paths";
import type { Doc, Documents, DocWatcher } from "./documents.svelte";

/** A server Thread knows how to run, as the backend describes it. */
export type ServerInfo = {
  id: string;
  name: string;
  /** The program it is, as it would be typed. */
  command: string;
  /** `name` is what the bottom bar calls the language, `id` what the protocol does. */
  languages: { name: string; id: string }[];
  /** A command that installs it. */
  install: string;
  installed: boolean;
};

export type ServerState = "starting" | "running" | "failed";

/** How long after a key before the server is told: once per burst of typing. */
const CHANGE_MS = 150;

/**
 * How long the keys have to have been still before what a server finds is
 * shown. A line being typed is wrong at nearly every keystroke -- a bracket
 * not closed yet, a statement with no end -- and being told so as it is
 * typed is noise.
 */
const QUIET_MS = 900;

type Attached = {
  server: string;
  /** Which of the server's processes: there is one for each folder. */
  process: string;
  client: LspClient;
  uri: string;
  version: number;
  /** Set while there is an edit the server has not been told of. */
  timer: ReturnType<typeof setTimeout> | null;
  /** When the file was last edited, as `Date.now()` has it. */
  edited: number;
  /** What the server last found, waiting for the keys to be still. */
  found: { items: Parameters<typeof toDiagnostic>[1][]; timer: ReturnType<typeof setTimeout> } | null;
};

// The list of completions, in the window's colours rather than the editor
// library's own light ones.
const look = EditorView.baseTheme({
  ".cm-tooltip": {
    backgroundColor: "var(--bg-menu)",
    border: "1px solid var(--border)",
    borderRadius: "5px",
    color: "var(--fg)",
    boxShadow: "0 8px 24px #0009",
  },
  ".cm-tooltip.cm-tooltip-autocomplete > ul": { fontFamily: "inherit", maxHeight: "16em" },
  ".cm-tooltip.cm-tooltip-autocomplete > ul > li": { padding: "2px 8px" },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    backgroundColor: "var(--hover-strong)",
    color: "var(--fg)",
  },
  ".cm-completionDetail": { color: "var(--fg-dim)", fontStyle: "normal", marginLeft: "1.5ch" },
  ".cm-completionMatchedText": { color: "var(--accent)", textDecoration: "none" },
  ".cm-tooltip.cm-completionInfo": { padding: "6px 8px", maxWidth: "28em", whiteSpace: "pre-wrap" },
});

export class Lsp implements DocWatcher {
  catalog = $state.raw<ServerInfo[]>([]);
  /** The servers switched on, by id: `[lsp] enabled` in the config. */
  enabled = $state.raw<string[]>([]);
  /** What each server that has been wanted is doing, by id. Absent: not wanted yet. */
  states = $state<Record<string, ServerState>>({});
  /** Why a server that failed did, by id. */
  errors = $state<Record<string, string>>({});

  /** Each server process, by server and folder; null once it would not start. */
  #clients = new Map<string, Promise<LspClient | null>>();
  #attached = new Map<number, Attached>();
  /** The latest attempt to attach each file, so an earlier one still waiting gives way. */
  #attempts = new Map<number, symbol>();

  constructor(
    private readonly docs: Documents,
    private readonly where: {
      /** The folders open in the tree. */
      roots: () => string[];
      /**
       * Whether there is a machine to run one on: not while the window is
       * between machines, or its connection has dropped.
       */
      ready: () => boolean;
    },
  ) {}

  /** Ask the machine the window is on which servers it has, and act on it. */
  async refresh() {
    try {
      this.catalog = await invoke<ServerInfo[]>("lsp_catalog");
    } catch (e) {
      console.error("lsp_catalog failed", e);
    }
    this.sync();
  }

  /** Take on the list of servers that are switched on. */
  configure(enabled: string[]) {
    if (JSON.stringify(enabled) === JSON.stringify(this.enabled)) return;
    this.enabled = enabled;
    for (const id of Object.keys(this.states)) if (!enabled.includes(id)) this.#stop(id);
    this.sync();
  }

  /** Start a server again that failed, or ended. */
  retry(id: string) {
    this.#stop(id);
    this.sync();
  }

  /** Bring every open file in line with what is installed and switched on. */
  sync() {
    for (const doc of this.docs.list) void this.#attach(doc);
  }

  /**
   * End every server, and forget which there are: the window is going to
   * another machine, or coming back to one it lost. Nothing starts again
   * until `refresh` has asked what that machine has.
   */
  reset() {
    for (const id of Object.keys(this.states)) this.#stop(id);
    this.catalog = [];
  }

  // --- what happens to the files ----------------------------------------------------

  opened(doc: Doc) {
    void this.#attach(doc);
  }

  moved(doc: Doc) {
    // A new path is a new file as far as a server is concerned, and a new
    // name can be a new language with a server of its own.
    void this.#attach(doc);
  }

  changed(key: number) {
    const attached = this.#attached.get(key);
    if (!attached) return;
    attached.timer ??= setTimeout(() => this.#flush(key), CHANGE_MS);
    attached.edited = Date.now();
    // Whatever was waiting to be shown waits again, from this key.
    if (attached.found) this.#found(key, attached.found.items);
  }

  /**
   * A server has had its say about a file. Shown once the file has gone
   * unedited for a moment, and at once if it already has.
   */
  #found(key: number, items: Parameters<typeof toDiagnostic>[1][]) {
    const attached = this.#attached.get(key);
    if (!attached) return;
    if (attached.found) clearTimeout(attached.found.timer);

    const show = () => {
      attached.found = null;
      const doc = this.docs.editor.doc(key);
      if (!doc || this.#attached.get(key) !== attached) return;
      this.docs.editor.keep(key, setDiagnostics.of(items.map((item) => toDiagnostic(doc, item))));
    };
    const wait = attached.edited + QUIET_MS - Date.now();
    attached.found = { items, timer: setTimeout(show, Math.max(0, wait)) };
  }

  saved(doc: Doc) {
    const attached = this.#attached.get(doc.key);
    if (!attached) return;
    this.#flush(doc.key);
    attached.client.notify("textDocument/didSave", { textDocument: { uri: attached.uri } });
  }

  closed(doc: Doc) {
    this.#attempts.delete(doc.key);
    this.#detach(doc.key, false);
  }

  // --- files and their servers ------------------------------------------------------

  /** The server for a file, and what it calls the file's language. */
  #serverFor(doc: Doc): { server: ServerInfo; language: string } | null {
    if (doc.path === null || !this.where.ready()) return null;
    const name = languageOf(doc.name);
    for (const server of this.catalog) {
      if (!server.installed || !this.enabled.includes(server.id)) continue;
      const language = server.languages.find((each) => each.name === name);
      if (language) return { server, language: language.id };
    }
    return null;
  }

  /** The project a file belongs to: the deepest open folder it is in, or its own. */
  #rootFor(path: string): string {
    const holding = this.where
      .roots()
      .filter((root) => segmentsBelow(root, path) !== null)
      .sort((a, b) => b.length - a.length)[0];
    return holding ?? dirName(path);
  }

  async #attach(doc: Doc) {
    const want = this.#serverFor(doc);
    const uri = doc.path === null ? null : toUri(doc.path);
    const have = this.#attached.get(doc.key);
    if (have && want && have.server === want.server.id && have.uri === uri) return;
    if (have) this.#detach(doc.key, true);

    const attempt = Symbol();
    this.#attempts.set(doc.key, attempt);
    if (!want || uri === null || doc.path === null) return;

    const process = `${want.server.id}\n${this.#rootFor(doc.path)}`;
    const client = await this.#client(want.server.id, process);
    // Closed, renamed or switched off while the server was starting.
    if (!client || this.#attempts.get(doc.key) !== attempt || !this.docs.find(doc.key)) return;

    this.#attached.set(doc.key, {
      server: want.server.id,
      process,
      client,
      uri,
      version: 1,
      timer: null,
      edited: 0,
      found: null,
    });
    client.notify("textDocument/didOpen", {
      textDocument: {
        uri,
        languageId: want.language,
        version: 1,
        text: this.docs.editor.text(doc.key),
      },
    });
    this.docs.editor.setTools(doc.key, this.#tools(doc.key));
  }

  /** `undress` is false for a file that is closing, which has nothing left to undress. */
  #detach(key: number, undress: boolean) {
    const attached = this.#attached.get(key);
    if (!attached) return;
    if (attached.timer !== null) clearTimeout(attached.timer);
    if (attached.found) clearTimeout(attached.found.timer);
    this.#attached.delete(key);
    attached.client.notify("textDocument/didClose", { textDocument: { uri: attached.uri } });
    if (undress) this.docs.editor.setTools(key, []);
  }

  /** Tell the server what a file says now, if it has not been told. */
  #flush(key: number) {
    const attached = this.#attached.get(key);
    if (!attached || attached.timer === null) return;
    clearTimeout(attached.timer);
    attached.timer = null;
    attached.client.notify("textDocument/didChange", {
      textDocument: { uri: attached.uri, version: ++attached.version },
      // The whole text each time. Every server takes that, and it cannot
      // drift out of step with the editor the way a run of small edits can.
      contentChanges: [{ text: this.docs.editor.text(key) }],
    });
  }

  // --- the servers themselves ---------------------------------------------------------

  #client(id: string, process: string): Promise<LspClient | null> {
    let pending = this.#clients.get(process);
    if (!pending) {
      if (this.states[id] !== "running") this.states[id] = "starting";
      const root = process.slice(process.indexOf("\n") + 1);
      pending = LspClient.start(id, root, {
        ondiagnostics: (uri, items) => {
          for (const [key, attached] of this.#attached) {
            if (attached.process !== process || uriKey(attached.uri) !== uriKey(uri)) continue;
            this.#found(key, items);
          }
        },
        onexit: () => this.#lost(id, process, "It stopped by itself."),
      }).then(
        (client) => {
          this.states[id] = "running";
          delete this.errors[id];
          return client;
        },
        (e) => {
          // Kept, as a server that would not start: asked again only when
          // someone says to, not with every file that is opened.
          this.states[id] = "failed";
          this.errors[id] = e instanceof Error ? e.message : String(e);
          return null;
        },
      );
      this.#clients.set(process, pending);
    }
    return pending;
  }

  /** A server process has gone without being asked to. */
  #lost(id: string, process: string, why: string) {
    for (const [key, attached] of this.#attached) {
      if (attached.process !== process) continue;
      if (attached.timer !== null) clearTimeout(attached.timer);
      this.#attached.delete(key);
      this.docs.editor.setTools(key, []);
    }
    this.#clients.set(process, Promise.resolve(null));
    this.states[id] = "failed";
    this.errors[id] = why;
  }

  /** End every process of a server, and forget it was ever wanted. */
  #stop(id: string) {
    for (const [key, attached] of this.#attached) {
      if (attached.server === id) this.#detach(key, true);
    }
    for (const [process, pending] of this.#clients) {
      if (!process.startsWith(`${id}\n`)) continue;
      this.#clients.delete(process);
      void pending.then((client) => client?.stop());
    }
    delete this.states[id];
    delete this.errors[id];
  }

  // --- what a server adds to a file -----------------------------------------------------

  #tools(key: number): Extension {
    const complete: CompletionSource = async (context) => {
      const attached = this.#attached.get(key);
      if (!attached) return null;

      const word = context.matchBefore(/[\w$]+/);
      const before = context.state.sliceDoc(Math.max(0, context.pos - 1), context.pos);
      const triggers = attached.client.capabilities.completionProvider?.triggerCharacters ?? [];
      // Mid-word, after a character the server asked to hear of (the dot of
      // `value.`), or because it was asked for. Not after every space.
      const triggered = !word && triggers.includes(before);
      if (!context.explicit && !word && !triggered) return null;

      // It has to have read what was just typed to complete it.
      this.#flush(key);
      let result: CompletionItem[] | { items?: CompletionItem[] } | null;
      try {
        result = await attached.client.request("textDocument/completion", {
          textDocument: { uri: attached.uri },
          position: toPosition(context.state.doc, context.pos),
          context: triggered ? { triggerKind: 2, triggerCharacter: before } : { triggerKind: 1 },
        });
      } catch {
        return null;
      }
      if (context.aborted || !result) return null;

      const items = Array.isArray(result) ? result : (result.items ?? []);
      if (items.length === 0) return null;
      return {
        from: word?.from ?? context.pos,
        options: toCompletions(items),
        // While only more of the word is typed, the list is narrowed here
        // rather than asked for again.
        validFor: /^[\w$]*$/,
      };
    };

    return [diagnostics, autocompletion({ override: [complete], icons: false }), look];
  }
}
