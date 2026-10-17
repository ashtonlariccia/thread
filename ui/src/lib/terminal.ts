/**
 * The terminal emulator: xterm.js, wired to this window's shell.
 *
 * Nothing imports this at the top level. xterm is most of a megabyte of
 * source that a window with no terminal open has no use for, so it is loaded
 * with the first terminal and not before.
 *
 * Drawn by xterm's DOM renderer unless the config asks for the WebGL one.
 * That is faster under a flood of output, and costs a GPU context and the
 * memory behind it for every terminal that is open — the wrong trade for a
 * tab that mostly runs a build, and the right one for tmux.
 *
 * Either way the shell is only read as fast as this draws: see `ACK_EVERY`.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { findLinks, type Link } from "./links";
import { THEME_EVENT } from "./themes";

export type TerminalLook = {
  fontFamily: string;
  fontSize: number;
  cursor: "block" | "bar" | "underline";
};

export type TerminalRenderer = "dom" | "webgl";

/**
 * How much output is drawn between one word to the backend that it has been
 * and the next. The backend stops reading a shell that is a few of these
 * ahead of the screen (`Flow`, in `terminal.rs`), which is what keeps a
 * program that prints faster than this can draw from burying the window.
 * Counted, not sent piece by piece: typing costs no messages at all.
 */
const ACK_EVERY = 32 * 1024;

export type TerminalOptions = TerminalLook & {
  /** What draws it. Read once, when it opens. */
  renderer: TerminalRenderer;
  /**
   * A name of its own, for as long as its tab is open. On a remote that
   * keeps terminals it is the name it is kept under, and so what it is come
   * back to by after the connection has dropped.
   */
  name: string;
  /** Where the shell starts; null leaves it to the backend. */
  cwd: string | null;
  scrollback: number;
  /** The shell has ended, by `exit` or by dying. Not called after `dispose`. */
  onexit: () => void;
  /** The command line of the shell to run; null leaves the choice to the config. */
  shell: string | null;
  /**
   * The names of every terminal the window has, this one's included: kept
   * ones that a remote's housekeeping is to leave alone, however long they
   * have sat there untouched.
   */
  keep: () => string[];
  /** A path or a web address in the output was Ctrl+clicked. */
  onlink: (link: Link) => void;
  /** Something run in the terminal asked for a file or a folder to be opened, or closed. */
  onopen: (kind: OpenKind, path: string) => void;
};

/**
 * What the `tide` command can ask for: a file opened in a tab, a folder
 * opened in the tree, or whichever of those a path is, closed.
 */
export type OpenKind = "file" | "dir" | "close";

/**
 * The escape sequence the `tide` command asks with (`scripts/tide-remote.sh`):
 * `OSC 7717 ; file|dir|close ; <absolute path> BEL`. A number nothing else uses.
 */
const OPEN_OSC = 7717;

/** What such a request asks for, or null if it is not one. */
export function openRequest(data: string): { kind: OpenKind; path: string } | null {
  const cut = data.indexOf(";");
  const kind = data.slice(0, cut);
  const path = data.slice(cut + 1);
  if (cut === -1 || path === "") return null;
  if (kind !== "file" && kind !== "dir" && kind !== "close") return null;
  return { kind, path };
}

export type TerminalHandle = {
  focus: () => void;
  hasSelection: () => boolean;
  /** Put the selection on the clipboard and drop the highlight. */
  copy: () => void;
  /** Type the clipboard at the shell. */
  paste: () => void;
  /** Type `text` at the shell, as if at the keyboard. */
  send: (text: string) => void;
  restyle: (look: TerminalLook) => void;
  /**
   * The connection is back. A terminal that was left on the remote when it
   * dropped is opened again; any other has nothing to do.
   */
  resume: () => void;
  /** End the shell and everything running in it. */
  dispose: () => void;
};

/**
 * Kept terminals whose tabs were closed while the connection was down, by
 * name: there was no one to tell that they were to end. They are told the
 * next time anything is opened, which is the next time there is a connection.
 */
const forgotten: string[] = [];

/** xterm's names for the sixteen colours, in the order of `--ansi-0` to `--ansi-15`. */
const ANSI = [
  "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
  "brightBlack", "brightRed", "brightGreen", "brightYellow",
  "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
] as const;

/** The window's own colours, read from where the theme defines them. */
export function theme(): ITheme {
  const style = getComputedStyle(document.documentElement);
  const color = (name: string) => style.getPropertyValue(name).trim();

  return {
    // Unpainted: a terminal sits on the same surface the editor does.
    background: "#00000000",
    foreground: color("--fg"),
    cursor: color("--caret"),
    cursorAccent: color("--accent-ink"),
    selectionBackground: color("--selection"),
    ...Object.fromEntries(ANSI.map((name, i) => [name, color(`--ansi-${i}`)])),
  };
}

/** Every terminal open, with what it adds to the theme, so a change of theme reaches it. */
const open = new Map<Terminal, Partial<ITheme>>();
let listening = false;

/**
 * Dress a terminal in the theme, now and whenever it changes. Gives back what
 * lets it go, for when it is disposed.
 */
export function themed(term: Terminal, extra: Partial<ITheme> = {}): () => void {
  term.options.theme = { ...theme(), ...extra };
  if (!listening) {
    // A frame later, so that the accent, which is put into effect after the
    // theme, has been too: the cursor's ink is made from it.
    window.addEventListener(THEME_EVENT, () => requestAnimationFrame(retheme));
    listening = true;
  }
  open.set(term, extra);
  return () => open.delete(term);
}

/** Put the theme now in effect onto every terminal that is open. */
function retheme() {
  const current = theme();
  for (const [term, extra] of open) term.options.theme = { ...current, ...extra };
}

/**
 * The clipboard's keys, for a terminal: what Windows has always had for
 * them, and the modern ones. Gives back the copy it does, for a menu to do.
 */
export function clipboardKeys(term: Terminal): () => void {
  const copy = () => {
    if (!term.hasSelection()) return;
    void writeText(term.getSelection()).catch((e) => console.error("copy failed", e));
    // Nothing else says the copy happened.
    term.clearSelection();
  };

  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown" || event.altKey) return true;
    const key = event.key.toLowerCase();

    // The keys Windows has always had for it, as well as the modern ones.
    if (key === "insert") {
      if (event.ctrlKey && !event.shiftKey) copy();
      // Shift+Insert, left alone, is a paste to the webview as Ctrl+V is.
      return !(event.ctrlKey || event.shiftKey);
    }
    if (!event.ctrlKey) return true;

    // Ctrl+C copies when something is selected, and is the interrupt it has
    // always been when nothing is. With Shift it only ever copies.
    if (key === "c" && (event.shiftKey || term.hasSelection())) {
      copy();
      return false;
    }
    // Ctrl+V pastes, whatever is running: a shell, or a full-screen program
    // such as an agent or an editor. Left alone, the webview pastes into
    // xterm's own text field, and that is the path that marks the text as a
    // paste for a program that asked to be told — which is what lets one
    // take several lines as a block instead of as so many presses of Enter.
    // (Vim's block select, Ctrl+V there, is also on Ctrl+Q.)
    if (key === "v") return false;
    return true;
  });
  return copy;
}

/**
 * Underline the paths and web addresses in a terminal's output under the
 * pointer, and follow one that is Ctrl+clicked. A plain click stays what it
 * is in a terminal: the start of a selection, or a click for the program.
 */
export function followLinks(term: Terminal, onlink: (link: Link) => void) {
  term.registerLinkProvider({
    provideLinks(row, callback) {
      const text = term.buffer.active.getLine(row - 1)?.translateToString(true) ?? "";
      const links = findLinks(text).map((link) => ({
        // Columns count from one, and the end is the last one in it.
        range: { start: { x: link.start + 1, y: row }, end: { x: link.end, y: row } },
        text: text.slice(link.start, link.end),
        activate: (event: MouseEvent) => {
          if (event.ctrlKey) onlink(link);
        },
      }));
      callback(links.length > 0 ? links : undefined);
    },
  });
}

/**
 * Leave a program's questions unanswered.
 *
 * A program asking its terminal something -- where the cursor is, what kind
 * of terminal it is, what colour the background is -- is asking tmux, and
 * tmux has answered by the time the question arrives here with the rest of
 * what the pane printed. An answer from xterm as well would be typed into
 * the pane as so much garbage after the prompt.
 */
export function staySilent(term: Terminal) {
  const quiet = () => true;
  const parser = term.parser;
  // Device status and cursor position, plain and DEC's.
  parser.registerCsiHandler({ final: "n" }, quiet);
  parser.registerCsiHandler({ prefix: "?", final: "n" }, quiet);
  // What kind of terminal this is, three ways.
  parser.registerCsiHandler({ final: "c" }, quiet);
  parser.registerCsiHandler({ prefix: ">", final: "c" }, quiet);
  parser.registerCsiHandler({ prefix: "=", final: "c" }, quiet);
  parser.registerCsiHandler({ prefix: ">", final: "q" }, quiet);
  // Whether a mode is set, and what a setting is.
  parser.registerCsiHandler({ intermediates: "$", final: "p" }, quiet);
  parser.registerCsiHandler({ prefix: "?", intermediates: "$", final: "p" }, quiet);
  parser.registerDcsHandler({ intermediates: "$", final: "q" }, quiet);
  parser.registerDcsHandler({ intermediates: "+", final: "q" }, quiet);
  // The window's size, in three units.
  parser.registerCsiHandler({ final: "t" }, (params) => {
    const what = params[0];
    return what === 14 || what === 16 || what === 18 || what === 19;
  });
  // A colour asked for is a question; a colour given is not, and is taken.
  for (const code of [4, 10, 11, 12]) {
    parser.registerOscHandler(code, (data) => data.split(";").includes("?"));
  }
}

export function openTerminal(host: HTMLElement, options: TerminalOptions): TerminalHandle {
  const term = new Terminal({
    fontFamily: options.fontFamily,
    fontSize: options.fontSize,
    cursorStyle: options.cursor,
    scrollback: options.scrollback,
    // Eased rather than switched on and off: see `TerminalView`'s styles.
    cursorBlink: true,
    // No hollow box while the editor has the keyboard: one caret at a time.
    cursorInactiveStyle: "none",
    allowTransparency: true,
  });
  const unthemed = themed(term);
  const fit = new FitAddon();
  term.loadAddon(fit);

  followLinks(term, options.onlink);
  term.open(host);
  fit.fit();

  let disposed = false;

  // Loaded only when asked for, as xterm itself is. A graphics context can
  // be refused, or taken away later; either leaves the DOM renderer, which
  // is what is drawing until this arrives anyway.
  if (options.renderer === "webgl") {
    void import("@xterm/addon-webgl").then(
      ({ WebglAddon }) => {
        if (disposed) return;
        try {
          const webgl = new WebglAddon();
          webgl.onContextLoss(() => webgl.dispose());
          term.loadAddon(webgl);
        } catch (e) {
          console.error("no WebGL for the terminal", e);
        }
      },
      (e) => console.error("no WebGL for the terminal", e),
    );
  }

  // `tide some-file`, typed at the shell: the command has no way to
  // reach this window but through the terminal it is run in, so it says
  // what it wants on the screen, in a sequence only this reads.
  term.parser.registerOscHandler(OPEN_OSC, (data) => {
    const request = openRequest(data);
    if (request) options.onopen(request.kind, request.path);
    return true;
  });

  /** Set while the shell is running; what `dispose` has to end. */
  let id: number | null = null;
  /**
   * Kept on the remote, and the connection has gone from under it: it is
   * still running there, and this is waiting to be told it can go back.
   */
  let waiting = false;
  /** Whether it has been told to leave a program's questions to tmux. */
  let silent = false;
  /** Counts the times it has been opened, so what an old one says late is not heard. */
  let opening = 0;

  const copy = clipboardKeys(term);

  // Held until the shell has an id to address it by. xterm answers the
  // console's opening questions the moment they arrive, which can be before
  // `terminal_open` has come back with one.
  let early: string[] = [];
  term.onData((data) => {
    if (id !== null) void invoke("terminal_write", { id, data });
    // Typed at a terminal that is out of reach, it is not kept to be sprung
    // on whatever is running there when the connection comes back.
    else if (!waiting) early.push(data);
  });
  term.onResize(({ cols, rows }) => {
    if (id !== null) void invoke("terminal_resize", { id, cols, rows });
  });

  /** Output drawn since the backend was last told. */
  let drawn = 0;
  const acknowledge = () => {
    if (id === null || drawn < ACK_EVERY) return;
    void invoke("terminal_drawn", { id, bytes: drawn });
    drawn = 0;
  };

  /** Start the shell, or go back to the one that was left running. */
  function open() {
    const attempt = ++opening;
    const again = waiting;

    const onOutput = new Channel<ArrayBuffer>();
    onOutput.onmessage = (bytes) => {
      if (attempt !== opening) return;
      const size = bytes.byteLength;
      // xterm takes what it is given and works through it in its own time;
      // the callback is when this piece has been.
      term.write(new Uint8Array(bytes), () => {
        drawn += size;
        acknowledge();
      });
    };
    const onExit = new Channel<boolean>();
    onExit.onmessage = (left) => {
      if (disposed || attempt !== opening) return;
      id = null;
      drawn = 0;
      if (!left) {
        options.onexit();
        return;
      }
      // Still running on the remote. The tab stays, with what was on it,
      // and says so; `resume` is what brings it back.
      waiting = true;
      // The connection may be back already, by the time this was heard: one
      // try before long, in case no one is left to say so. If it is not
      // back, the try comes to nothing and it goes on waiting.
      setTimeout(() => {
        if (waiting && !disposed) open();
      }, 1500);
      term.write("\r\n\x1b[2m[connection lost \u2014 still running on the remote]\x1b[0m\r\n");
    };

    invoke<{ id: number; kept: boolean }>("terminal_open", {
      onOutput,
      onExit,
      name: options.name,
      cwd: options.cwd,
      shell: options.shell,
      cols: term.cols,
      rows: term.rows,
      keep: options.keep(),
    }).then(
      (opened) => {
        if (disposed || attempt !== opening) {
          void invoke("terminal_close", { id: opened.id });
          return;
        }
        id = opened.id;
        waiting = false;
        // A kept terminal is tmux's pane: what a program asks of its
        // terminal, tmux has answered (see `staySilent`).
        if (opened.kept && !silent) {
          silent = true;
          staySilent(term);
        }
        if (early.length > 0) void invoke("terminal_write", { id, data: early.join("") });
        early = [];
        // What was drawn before there was an id to say so by.
        acknowledge();
        // The window may have been resized while the shell was starting.
        void invoke("terminal_resize", { id, cols: term.cols, rows: term.rows });
        // There is a connection, so there is someone to tell.
        if (forgotten.length > 0) void invoke("terminal_forget", { names: forgotten.splice(0) });
      },
      (e) => {
        if (disposed || attempt !== opening) return;
        // Going back to one that was left: not yet, then. It is still
        // waiting, and will be told again.
        if (again) return;
        // Left on screen rather than closed, so there is something to read.
        term.write(`\x1b[31m${String(e).replaceAll("\n", "\r\n")}\x1b[0m\r\n`);
      },
    );
  }
  open();

  // Debounced: a drag reports every pixel, and each new size is a redraw of
  // the whole screen for the program on the other end.
  let resizeTimer: ReturnType<typeof setTimeout> | undefined;
  const refit = () => {
    // Nothing can be measured while it is not laid out.
    if (host.clientWidth > 0 && host.clientHeight > 0) fit.fit();
  };
  const observer = new ResizeObserver(() => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(refit, 60);
  });
  observer.observe(host);

  return {
    focus: () => term.focus(),
    hasSelection: () => term.hasSelection(),
    copy,
    paste: () => {
      // Through xterm rather than straight to the shell: it turns line
      // endings into Enter and brackets the text if the program asked.
      void readText().then(
        (text) => term.paste(text),
        (e) => console.error("paste failed", e),
      );
    },
    send: (text) => term.input(text),
    restyle: (look) => {
      term.options.fontFamily = look.fontFamily;
      term.options.fontSize = look.fontSize;
      term.options.cursorStyle = look.cursor;
      refit();
    },
    resume: () => {
      if (waiting && !disposed) open();
    },
    dispose: () => {
      disposed = true;
      clearTimeout(resizeTimer);
      observer.disconnect();
      if (id !== null) void invoke("terminal_close", { id });
      // Out of reach, it cannot be told to end; it is remembered until it can.
      else if (waiting) forgotten.push(options.name);
      unthemed();
      term.dispose();
    },
  };
}
