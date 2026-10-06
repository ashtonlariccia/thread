/**
 * The terminal emulator: xterm.js, wired to this window's shell.
 *
 * Nothing imports this at the top level. xterm is most of a megabyte of
 * source that a window with no terminal open has no use for, so it is loaded
 * with the first terminal and not before.
 *
 * Drawn by xterm's DOM renderer. The WebGL one is faster under a flood of
 * output, and costs a GPU context and the memory behind it for every
 * terminal that is open — the wrong trade for a tab that mostly runs a build.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal, type ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

export type TerminalLook = {
  fontFamily: string;
  fontSize: number;
  cursor: "block" | "bar" | "underline";
};

export type TerminalOptions = TerminalLook & {
  /** Where the shell starts; null leaves it to the backend. */
  cwd: string | null;
  scrollback: number;
  /** The shell has ended, by `exit` or by dying. Not called after `dispose`. */
  onexit: () => void;
  /** Something run in the terminal asked for a file or a folder to be opened. */
  onopen: (kind: "file" | "dir", path: string) => void;
};

/**
 * The escape sequence the `thread` command asks with (`scripts/thread-remote.sh`):
 * `OSC 7717 ; file|dir ; <absolute path> BEL`. A number nothing else uses.
 */
const OPEN_OSC = 7717;

/** What such a request asks for, or null if it is not one. */
export function openRequest(data: string): { kind: "file" | "dir"; path: string } | null {
  const cut = data.indexOf(";");
  const kind = data.slice(0, cut);
  const path = data.slice(cut + 1);
  if (cut === -1 || (kind !== "file" && kind !== "dir") || path === "") return null;
  return { kind, path };
}

export type TerminalHandle = {
  focus: () => void;
  hasSelection: () => boolean;
  /** Put the selection on the clipboard and drop the highlight. */
  copy: () => void;
  /** Type the clipboard at the shell. */
  paste: () => void;
  restyle: (look: TerminalLook) => void;
  /** End the shell and everything running in it. */
  dispose: () => void;
};

/**
 * The window's own colours, read from where they are defined. The sixteen
 * below are Catppuccin's, with its blue-greys swapped for the neutral ones
 * the rest of this window uses.
 */
function theme(): ITheme {
  const style = getComputedStyle(document.documentElement);
  const color = (name: string) => style.getPropertyValue(name).trim();

  return {
    // Unpainted: a terminal sits on the same surface the editor does.
    background: "#00000000",
    foreground: color("--fg"),
    cursor: color("--caret"),
    cursorAccent: color("--accent-ink"),
    selectionBackground: color("--selection"),
    black: "#454545",
    red: "#f38ba8",
    green: "#a6e3a1",
    yellow: "#f9e2af",
    blue: "#89b4fa",
    magenta: "#cba6f7",
    cyan: "#94e2d5",
    white: "#bababa",
    // What a shell dims its suggestions with, so it has to stay readable.
    brightBlack: "#7a7a7a",
    brightRed: "#f38ba8",
    brightGreen: "#a6e3a1",
    brightYellow: "#f9e2af",
    brightBlue: "#89b4fa",
    brightMagenta: "#cba6f7",
    brightCyan: "#94e2d5",
    brightWhite: "#d8d8d8",
  };
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
    theme: theme(),
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(host);
  fit.fit();

  // `thread some-file`, typed at the shell: the command has no way to
  // reach this window but through the terminal it is run in, so it says
  // what it wants on the screen, in a sequence only this reads.
  term.parser.registerOscHandler(OPEN_OSC, (data) => {
    const request = openRequest(data);
    if (request) options.onopen(request.kind, request.path);
    return true;
  });

  let disposed = false;
  /** Set once the shell is running; what `dispose` has to end. */
  let id: number | null = null;

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

  // Held until the shell has an id to address it by. xterm answers the
  // console's opening questions the moment they arrive, which can be before
  // `terminal_open` has come back with one.
  const early: string[] = [];
  term.onData((data) => {
    if (id === null) early.push(data);
    else void invoke("terminal_write", { id, data });
  });
  term.onResize(({ cols, rows }) => {
    if (id !== null) void invoke("terminal_resize", { id, cols, rows });
  });

  const onOutput = new Channel<ArrayBuffer>();
  onOutput.onmessage = (bytes) => term.write(new Uint8Array(bytes));
  const onExit = new Channel<null>();
  onExit.onmessage = () => {
    if (!disposed) options.onexit();
  };

  invoke<number>("terminal_open", {
    onOutput,
    onExit,
    cwd: options.cwd,
    cols: term.cols,
    rows: term.rows,
  }).then(
    (opened) => {
      if (disposed) {
        void invoke("terminal_close", { id: opened });
        return;
      }
      id = opened;
      if (early.length > 0) void invoke("terminal_write", { id, data: early.join("") });
      // The window may have been resized while the shell was starting.
      void invoke("terminal_resize", { id, cols: term.cols, rows: term.rows });
    },
    (e) => {
      // Left on screen rather than closed, so there is something to read.
      if (!disposed) term.write(`\x1b[31m${String(e).replaceAll("\n", "\r\n")}\x1b[0m\r\n`);
    },
  );

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
    restyle: (look) => {
      term.options.fontFamily = look.fontFamily;
      term.options.fontSize = look.fontSize;
      term.options.cursorStyle = look.cursor;
      refit();
    },
    dispose: () => {
      disposed = true;
      clearTimeout(resizeTimer);
      observer.disconnect();
      if (id !== null) void invoke("terminal_close", { id });
      term.dispose();
    },
  };
}
