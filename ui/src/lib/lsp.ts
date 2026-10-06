/**
 * The Language Server Protocol, as much of it as Thread speaks: starting a
 * server, telling it what is in the open files, asking it what could be
 * typed next, and hearing what it finds wrong.
 *
 * The backend runs the server and carries whole messages to and from it
 * (`lsp.rs`). Everything about what the messages mean is here.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { Completion } from "@codemirror/autocomplete";
import type { Text } from "@codemirror/state";

import type { Diagnostic, Severity } from "./diagnostics";

/** A place in a file as the protocol says it: both counted from zero. */
export type Position = { line: number; character: number };
export type Range = { start: Position; end: Position };

type TextEdit = { range: Range; newText: string };

/** A completion as a server sends it; only what is read here. */
export type CompletionItem = {
  label: string;
  kind?: number;
  detail?: string;
  documentation?: string | { value: string };
  sortText?: string;
  filterText?: string;
  insertText?: string;
  textEdit?: TextEdit | { insert: Range; replace: Range; newText: string };
  additionalTextEdits?: TextEdit[];
};

type WireDiagnostic = { range: Range; severity?: number; message: string; source?: string };

// --- paths and places -------------------------------------------------------------

/** A path on this machine as a `file:` URI. */
export function toUri(path: string): string {
  const slashed = path.replaceAll("\\", "/");
  const parts = slashed.split("/").map((part, index) =>
    // The drive keeps its colon: `file:///C:/…` is how they are written.
    index === 0 && /^[a-zA-Z]:$/.test(part) ? part : encodeURIComponent(part),
  );
  return `file://${slashed.startsWith("/") ? "" : "/"}${parts.join("/")}`;
}

/**
 * A URI reduced to what identifies the file. Servers send back the ones
 * they were given in spellings of their own: the drive in the other case,
 * its colon escaped.
 */
export function uriKey(uri: string): string {
  try {
    return decodeURIComponent(uri).toLowerCase();
  } catch {
    return uri.toLowerCase();
  }
}

export function toPosition(doc: Text, offset: number): Position {
  const line = doc.lineAt(offset);
  return { line: line.number - 1, character: offset - line.from };
}

/** The offset of a position, kept inside the text: a server can be a version behind. */
export function toOffset(doc: Text, position: Position): number {
  if (position.line >= doc.lines) return doc.length;
  const line = doc.line(Math.max(0, position.line) + 1);
  return line.from + Math.min(Math.max(0, position.character), line.length);
}

const SEVERITY: Record<number, Severity> = { 1: "error", 2: "warning", 3: "info", 4: "hint" };

export function toDiagnostic(doc: Text, wire: WireDiagnostic): Diagnostic {
  return {
    from: toOffset(doc, wire.range.start),
    to: toOffset(doc, wire.range.end),
    // One that does not say is taken to be serious.
    severity: SEVERITY[wire.severity ?? 1] ?? "error",
    message: wire.message,
    source: wire.source,
  };
}

/** `CompletionItemKind`, as the editor names the same things. */
const KINDS: Record<number, string> = {
  2: "method",
  3: "function",
  4: "function",
  5: "property",
  6: "variable",
  7: "class",
  8: "interface",
  9: "namespace",
  10: "property",
  11: "constant",
  12: "constant",
  13: "enum",
  14: "keyword",
  20: "constant",
  21: "constant",
  22: "class",
  25: "type",
};

/**
 * A server's completions as the editor's. They come ranked, by `sortText`,
 * and that ranking is kept among the ones that match what was typed.
 */
export function toCompletions(items: CompletionItem[]): Completion[] {
  const ranked = [...items].sort((a, b) =>
    (a.sortText ?? a.label) < (b.sortText ?? b.label) ? -1 : 1,
  );
  return ranked.map((item, rank): Completion => {
    const text = item.textEdit?.newText ?? item.insertText ?? item.label;
    const extra = item.additionalTextEdits ?? [];
    const range = item.textEdit && ("range" in item.textEdit ? item.textEdit.range : item.textEdit.replace);
    const documentation =
      typeof item.documentation === "string" ? item.documentation : item.documentation?.value;

    return {
      label: item.label,
      detail: item.detail,
      info: documentation || undefined,
      type: item.kind === undefined ? undefined : KINDS[item.kind],
      boost: Math.max(-99, 99 - rank),
      apply:
        !range && extra.length === 0
          ? text
          : (view, _completion, from, to) => {
              const doc = view.state.doc;
              // Where the server said, but never short of what was matched:
              // more may have been typed since it was asked.
              const start = range ? Math.min(toOffset(doc, range.start), from) : from;
              const changes = view.state.changes([
                { from: start, to, insert: text },
                // What the completion needs elsewhere: an import, usually.
                ...extra.map((edit) => ({
                  from: toOffset(doc, edit.range.start),
                  to: toOffset(doc, edit.range.end),
                  insert: edit.newText,
                })),
              ]);
              view.dispatch({
                changes,
                selection: { anchor: changes.mapPos(to, 1) },
                scrollIntoView: true,
                userEvent: "input.complete",
              });
            },
    };
  });
}

// --- the client ---------------------------------------------------------------------

type Json = Record<string, unknown>;

export type ClientEvents = {
  /** Everything the server now has to say about one file. */
  ondiagnostics: (uri: string, diagnostics: WireDiagnostic[]) => void;
  /** The server has ended, without being asked to. */
  onexit: () => void;
};

/** What the server said it can do, as much of it as is asked about. */
export type Capabilities = {
  completionProvider?: { triggerCharacters?: string[] };
};

/** How long a server is given to answer before the question is dropped. */
const REQUEST_MS = 10_000;

/** One running server, and the conversation with it. */
export class LspClient {
  capabilities: Capabilities = {};

  #id: number;
  #next = 1;
  #pending = new Map<number, { resolve: (result: unknown) => void; reject: (e: Error) => void }>();
  #stopped = false;

  private constructor(
    id: number,
    private readonly events: ClientEvents,
  ) {
    this.#id = id;
  }

  /**
   * Start a server in `root` and get through the opening exchange with it.
   * Rejects, with the server ended, if either does not happen.
   */
  static async start(server: string, root: string | null, events: ClientEvents): Promise<LspClient> {
    // Made before the server is: it may speak before `lsp_start` has answered.
    let client: LspClient | null = null;
    const early: string[] = [];
    const onMessage = new Channel<string>();
    onMessage.onmessage = (message) => {
      if (client) client.#receive(message);
      else early.push(message);
    };
    const onExit = new Channel<null>();
    onExit.onmessage = () => {
      if (client) client.#ended();
    };

    const id = await invoke<number>("lsp_start", { server, cwd: root, onMessage, onExit });
    client = new LspClient(id, events);
    for (const message of early) client.#receive(message);

    try {
      const result = await client.request<{ capabilities?: Capabilities }>("initialize", {
        processId: null,
        clientInfo: { name: "Thread" },
        rootUri: root === null ? null : toUri(root),
        workspaceFolders:
          root === null ? null : [{ uri: toUri(root), name: root.split(/[\\/]/).at(-1) ?? root }],
        capabilities: {
          general: { positionEncodings: ["utf-16"] },
          textDocument: {
            synchronization: { didSave: true },
            completion: {
              contextSupport: true,
              // Plain text in, plain text out: no snippets with holes to tab
              // through, and no markdown to render.
              completionItem: { snippetSupport: false, documentationFormat: ["plaintext"] },
            },
            publishDiagnostics: {},
          },
          workspace: { workspaceFolders: true, configuration: true },
        },
      });
      client.capabilities = result?.capabilities ?? {};
      client.notify("initialized", {});
      return client;
    } catch (e) {
      client.stop();
      throw e;
    }
  }

  #send(message: Json) {
    if (this.#stopped) return;
    void invoke("lsp_send", { id: this.#id, message: JSON.stringify({ jsonrpc: "2.0", ...message }) });
  }

  /** Ask the server something. Rejects if it refuses, ends, or takes too long. */
  request<T>(method: string, params: unknown): Promise<T> {
    const id = this.#next++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`${method} went unanswered`));
      }, REQUEST_MS);
      this.#pending.set(id, {
        resolve: (result) => {
          clearTimeout(timer);
          resolve(result as T);
        },
        reject: (e) => {
          clearTimeout(timer);
          reject(e);
        },
      });
      this.#send({ id, method, params });
    });
  }

  /** Tell the server something that needs no answer. */
  notify(method: string, params: unknown) {
    this.#send({ method, params });
  }

  #receive(text: string) {
    let message: Json;
    try {
      message = JSON.parse(text) as Json;
    } catch {
      return;
    }

    if (typeof message.method === "string") {
      if (message.id === undefined) this.#notified(message.method, message.params);
      else this.#asked(message.id, message.method, message.params);
      return;
    }
    if (typeof message.id !== "number") return;
    const pending = this.#pending.get(message.id);
    if (!pending) return;
    this.#pending.delete(message.id);
    const error = message.error as { message?: string } | undefined;
    if (error) pending.reject(new Error(error.message ?? "the server refused"));
    else pending.resolve(message.result);
  }

  #notified(method: string, params: unknown) {
    if (method !== "textDocument/publishDiagnostics") return;
    const { uri, diagnostics } = params as { uri: string; diagnostics: WireDiagnostic[] };
    this.events.ondiagnostics(uri, diagnostics ?? []);
  }

  /**
   * The server wants something of us. A server waits on these, so each gets
   * an answer, if only that there is nothing to say.
   */
  #asked(id: unknown, method: string, params: unknown) {
    if (method === "workspace/configuration") {
      // No settings of ours for it: one "nothing" for each it asked after.
      const items = (params as { items?: unknown[] } | undefined)?.items ?? [];
      this.#send({ id, result: items.map(() => null) });
    } else if (method === "client/registerCapability" || method === "window/workDoneProgress/create") {
      this.#send({ id, result: null });
    } else {
      this.#send({ id, error: { code: -32601, message: `${method} is not something Thread does` } });
    }
  }

  #ended() {
    if (this.#stopped) return;
    this.#stopped = true;
    for (const pending of this.#pending.values()) pending.reject(new Error("the server ended"));
    this.#pending.clear();
    this.events.onexit();
  }

  /** End the server. `onexit` is not called: this was asked for. */
  stop() {
    if (this.#stopped) return;
    // Its chance to put its things away; the backend ends it either way.
    this.notify("exit", null);
    this.#stopped = true;
    for (const pending of this.#pending.values()) pending.reject(new Error("the server was stopped"));
    this.#pending.clear();
    void invoke("lsp_stop", { id: this.#id });
  }
}
