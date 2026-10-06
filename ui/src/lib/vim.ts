/**
 * Vim motions, by way of `@replit/codemirror-vim`.
 *
 * Fetched only when vim mode is switched on: it is a whole editor's worth of
 * key handling, and nobody who has it off should pay for it.
 *
 * The extension brings the motions, operators, registers, macros, visual
 * modes and the `:` command line. What is added here is the join to Thread:
 * the ex commands that mean something outside the buffer (`:w`, `:q`), and
 * reporting the mode so the bottom bar can show it.
 */
import type { Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

import type { SplitDir } from "./panes";

export type VimMode = "normal" | "insert" | "visual" | "replace";

/** What the ex commands that reach outside the buffer should do. */
export type VimHooks = {
  write: () => void;
  /** `force` is `:q!`: close the file and lose what is unsaved, unasked. */
  quit: (force: boolean) => void;
  writeQuit: () => void;
  /** `:bn` / `:bp`: the next or previous tab of the pane. */
  cycle: (step: 1 | -1) => void;
  /**
   * `:sp` and `:vsp`: another pane, below or beside this one. On the file
   * named after the command, or with none named a second view of this one.
   */
  split: (dir: SplitDir, file: string | null) => void;
  /** `:new` and `:vnew`: another pane, on a new empty file. */
  fresh: (dir: SplitDir) => void;
  /** `:e`: open a file in this pane. */
  edit: (file: string) => void;
  /** `:close`: close this pane, and keep what was in it. */
  close: () => void;
  /** `:only`: close every pane but this one. */
  only: () => void;
  /** `:wincmd`: `h` `j` `k` `l` to the pane that way, `w` and `p` to step through them. */
  wincmd: (arg: string) => void;
  /** `:qa`: close every file, in every pane. `force` is `:qa!`. */
  quitAll: (force: boolean) => void;
  /** `:wa`: save every file that has unsaved changes. */
  writeAll: () => void;
  /** `:wqa` and `:xa`: save every file, and close them all if they all saved. */
  writeQuitAll: () => void;
  /**
   * `u` and Ctrl+R, in the editor they were pressed in. The history is the
   * file's and not that editor's, so the extension's own way of undoing,
   * which looks in the editor, would find nothing there.
   */
  undo: (view: EditorView) => void;
  redo: (view: EditorView) => void;
};

export type VimApi = {
  extension: Extension;
  /**
   * Follow whatever is in `view` now: report its mode, and show its command
   * line and messages in `line()`, a bar the window draws under the text,
   * rather than in the extension's own panel. Call
   * again whenever the view is given a different file or its extensions
   * change: the object the events come from is the view's, and may have been
   * replaced.
   */
  watch: (
    view: EditorView,
    onmode: (mode: VimMode) => void,
    line: () => HTMLElement | null,
  ) => void;
};

/** What the extension passes with `vim-mode-change`. */
type ModeChange = { mode: string; subMode?: string };

function modeOf(change: ModeChange): VimMode {
  if (change.mode === "insert" || change.mode === "replace") return change.mode;
  return change.mode === "visual" ? "visual" : "normal";
}

/** How long a message from vim stays up if no key is pressed. */
const MESSAGE_MS = 4000;

let loading: Promise<VimApi> | null = null;
// The ex commands are defined once, globally, by the extension; they reach
// whichever window state is current through here.
let hooks: VimHooks | null = null;

export function loadVim(next: VimHooks): Promise<VimApi> {
  hooks = next;
  loading ??= import("@replit/codemirror-vim").then(({ vim, Vim, getCM, CodeMirror }) => {
    // Every way the extension has of undoing goes through these two.
    CodeMirror.commands.undo = (cm) => hooks?.undo(cm.cm6);
    CodeMirror.commands.redo = (cm) => hooks?.redo(cm.cm6);

    // Patterns are Vim's, not JavaScript's: `\(a\|b\)` groups and `\+`
    // repeats, with a bare `(` or `+` meaning itself. The extension defaults
    // to JavaScript syntax (and says so after every search and substitute);
    // this is the switch it offers for the real thing. `vimSubstitute.ts`
    // reads patterns the same way, so the preview and the command agree.
    Vim.setOption("pcre", false);

    // These run once the command line that asked for them has closed, not
    // from inside it. Closing a file takes its editor state with it, and the
    // extension still has its own line to put away in that state: done in the
    // other order, the line was left on screen over a file that had gone.
    const later = (act: () => void) => () => void setTimeout(act, 0);
    /** Whether a command was given with a `!`, which arrives as its argument. */
    const banged = (params?: { argString?: string }) =>
      params?.argString?.trim().startsWith("!") ?? false;

    // Each takes its usual abbreviation: `:w` for `:write`, and so on.
    Vim.defineEx("write", "w", later(() => hooks?.write()));
    Vim.defineEx("quit", "q", (_cm: unknown, params?: { argString?: string }) => {
      const force = banged(params);
      later(() => hooks?.quit(force))();
    });
    Vim.defineEx("wq", "wq", later(() => hooks?.writeQuit()));
    Vim.defineEx("xit", "x", later(() => hooks?.writeQuit()));
    Vim.defineEx("bnext", "bn", later(() => hooks?.cycle(1)));
    Vim.defineEx("bprevious", "bp", later(() => hooks?.cycle(-1)));

    // Windows, which here are panes. `:sp` is a split with one pane above
    // the other, and `:vsp` one with them side by side.
    type Params = { argString?: string };
    const named = (params?: Params) => params?.argString?.trim() || null;
    const withArg =
      (act: (arg: string | null) => void) => (_cm: unknown, params?: Params) => {
        const arg = named(params);
        later(() => act(arg))();
      };
    Vim.defineEx("split", "sp", withArg((file) => hooks?.split("column", file)));
    Vim.defineEx("vsplit", "vs", withArg((file) => hooks?.split("row", file)));
    Vim.defineEx("new", "new", later(() => hooks?.fresh("column")));
    Vim.defineEx("vnew", "vne", later(() => hooks?.fresh("row")));
    Vim.defineEx("edit", "e", withArg((file) => file !== null && hooks?.edit(file)));
    Vim.defineEx("close", "clo", later(() => hooks?.close()));
    Vim.defineEx("only", "on", later(() => hooks?.only()));
    Vim.defineEx("wincmd", "winc", withArg((arg) => arg !== null && hooks?.wincmd(arg)));

    // Every file at once.
    Vim.defineEx("qall", "qa", (_cm: unknown, params?: Params) => {
      const force = banged(params);
      later(() => hooks?.quitAll(force))();
    });
    Vim.defineEx("wall", "wa", later(() => hooks?.writeAll()));
    Vim.defineEx("wqall", "wqa", later(() => hooks?.writeQuitAll()));
    Vim.defineEx("xall", "xa", later(() => hooks?.writeQuitAll()));

    const watched = new WeakSet<object>();
    return {
      // No status panel of its own: the bottom bar shows the mode.
      extension: vim({ status: false }),
      watch(view, onmode, line) {
        const cm = getCM(view);
        if (!cm) return;
        if (!watched.has(cm)) {
          watched.add(cm);
          cm.on("vim-mode-change", (change: ModeChange) => onmode(modeOf(change)));

          // The extension shows its `:` line, `/` search and messages in a
          // panel under the text, by putting one element -- `state.dialog` --
          // into it and announcing the change. That element is self-contained
          // (its own key handlers, its own focus), so it works wherever it is
          // put. This runs after the extension's own handler, and moves it.
          cm.on("dialog", () => {
            const host = line();
            if (!host) return;
            const dialog = cm.state.dialog as HTMLElement | null | undefined;
            if (!dialog) {
              host.replaceChildren();
              return;
            }
            host.replaceChildren(dialog);

            // A prompt goes when it is answered. A message has nothing to
            // answer, and the extension leaves it up until the next prompt,
            // holding a line of the editor's height for something already
            // read. So it goes at the next key, or by itself after a moment.
            if (dialog.querySelector("input")) return;
            const dismiss = () => {
              clearTimeout(timer);
              view.contentDOM.removeEventListener("keydown", dismiss);
              if (dialog.parentElement === host) host.replaceChildren();
            };
            const timer = setTimeout(dismiss, MESSAGE_MS);
            view.contentDOM.addEventListener("keydown", dismiss);
          });
        }
        // And where it stands right now, which no event is going to announce.
        const state = cm.state.vim;
        onmode(state?.insertMode ? "insert" : state?.visualMode ? "visual" : "normal");
      },
    };
  });
  // Let a later attempt retry rather than wedging on a failed fetch.
  loading.catch(() => (loading = null));
  return loading;
}
