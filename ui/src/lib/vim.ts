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

export type VimMode = "normal" | "insert" | "visual" | "replace";

/** What the ex commands that reach outside the buffer should do. */
export type VimHooks = {
  write: () => void;
  quit: () => void;
  writeQuit: () => void;
  /** `:bn` / `:bp`: the next or previous open file. */
  cycle: (step: 1 | -1) => void;
};

export type VimApi = {
  extension: Extension;
  /**
   * Follow whatever is in `view` now: report its mode, and show its command
   * line and messages in `line()` rather than in a panel under the text. Call
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

let loading: Promise<VimApi> | null = null;
// The ex commands are defined once, globally, by the extension; they reach
// whichever window state is current through here.
let hooks: VimHooks | null = null;

export function loadVim(next: VimHooks): Promise<VimApi> {
  hooks = next;
  loading ??= import("@replit/codemirror-vim").then(({ vim, Vim, getCM }) => {
    // Patterns are Vim's, not JavaScript's: `\(a\|b\)` groups and `\+`
    // repeats, with a bare `(` or `+` meaning itself. The extension defaults
    // to JavaScript syntax (and says so after every search and substitute);
    // this is the switch it offers for the real thing. `vimSubstitute.ts`
    // reads patterns the same way, so the preview and the command agree.
    Vim.setOption("pcre", false);

    // Each takes its usual abbreviation: `:w` for `:write`, and so on.
    Vim.defineEx("write", "w", () => hooks?.write());
    Vim.defineEx("quit", "q", () => hooks?.quit());
    Vim.defineEx("wq", "wq", () => hooks?.writeQuit());
    Vim.defineEx("xit", "x", () => hooks?.writeQuit());
    Vim.defineEx("bnext", "bn", () => hooks?.cycle(1));
    Vim.defineEx("bprevious", "bp", () => hooks?.cycle(-1));

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
            if (dialog) host.replaceChildren(dialog);
            else host.replaceChildren();
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
