/**
 * One tmux pane, drawn: xterm.js showing what tmux says the pane printed.
 *
 * Unlike a terminal in the editor's tabs, nothing here owns a shell or
 * decides a size. tmux has the shell, and tmux says how many columns and
 * rows the pane is; this is given both and draws them.
 *
 * Loaded with the first pane and not before, as `terminal.ts` is.
 */
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import type { Link } from "../links";
import {
  clipboardKeys,
  followLinks,
  staySilent,
  themed,
  type TerminalLook,
  type TerminalRenderer,
} from "../terminal";

export type PaneOptions = TerminalLook & {
  renderer: TerminalRenderer;
  scrollback: number;
  cols: number;
  rows: number;
  /** What was typed, or pasted, as the emulator encoded it. */
  oninput: (text: string) => void;
  /** The same, for what is not text. */
  onbytes: (bytes: number[]) => void;
  onlink: (link: Link) => void;
};

export type PaneHandle = {
  /** Draw what the pane printed. `done` runs once it has been. */
  write: (bytes: Uint8Array, done: () => void) => void;
  /** tmux has given the pane a new size. Takes effect after what is already written. */
  resize: (cols: number, rows: number) => void;
  focus: () => void;
  hasSelection: () => boolean;
  copy: () => void;
  paste: () => void;
  restyle: (look: TerminalLook) => void;
  dispose: () => void;
};

/** How wide a pane's scrollbar is, in pixels. */
const SCROLLBAR = 6;

/** The WebGL renderer, where the config asks for it and the machine allows it. */
function accelerate(term: Terminal, gone: () => boolean) {
  void import("@xterm/addon-webgl").then(
    ({ WebglAddon }) => {
      if (gone()) return;
      try {
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => webgl.dispose());
        term.loadAddon(webgl);
      } catch (e) {
        console.error("no WebGL for the pane", e);
      }
    },
    (e) => console.error("no WebGL for the pane", e),
  );
}

export function openPane(host: HTMLElement, options: PaneOptions): PaneHandle {
  const term = new Terminal({
    cols: options.cols,
    rows: options.rows,
    fontFamily: options.fontFamily,
    fontSize: options.fontSize,
    cursorStyle: options.cursor,
    scrollback: options.scrollback,
    cursorBlink: true,
    cursorInactiveStyle: "none",
    allowTransparency: true,
    // This is what sets how wide xterm's scrollbar is. A pane is exactly as
    // wide as its columns, so the bar lies over the last of them while it
    // shows: at its usual width that is two columns of text, and at this,
    // most of one.
    overviewRuler: { width: SCROLLBAR },
  });
  const unthemed = themed(term, { overviewRulerBorder: "#00000000" });
  followLinks(term, options.onlink);
  staySilent(term);
  term.open(host);

  let disposed = false;
  if (options.renderer === "webgl") accelerate(term, () => disposed);
  const copy = clipboardKeys(term);

  term.onData(options.oninput);
  term.onBinary((data) => options.onbytes(Array.from(data, (char) => char.charCodeAt(0) & 0xff)));

  return {
    write: (bytes, done) => term.write(bytes, done),
    resize: (cols, rows) => {
      // Behind what has been written and not yet drawn: that was printed
      // for the size the pane had, and is to be drawn at it.
      term.write("", () => {
        if (!disposed && (term.cols !== cols || term.rows !== rows)) term.resize(cols, rows);
      });
    },
    focus: () => term.focus(),
    hasSelection: () => term.hasSelection(),
    copy,
    paste: () => {
      void readText().then(
        (text) => term.paste(text),
        (e) => console.error("paste failed", e),
      );
    },
    restyle: (look) => {
      term.options.fontFamily = look.fontFamily;
      term.options.fontSize = look.fontSize;
      term.options.cursorStyle = look.cursor;
    },
    dispose: () => {
      disposed = true;
      unthemed();
      term.dispose();
    },
  };
}

/** The size of one character cell, in CSS pixels. */
export type CellSize = { w: number; h: number };

export type Probe = {
  /** What a cell measures now, or null if it cannot be told yet. */
  cell: () => CellSize | null;
  restyle: (look: TerminalLook) => void;
  dispose: () => void;
};

/**
 * A terminal that is never seen, for measuring with.
 *
 * tmux has to be told how many columns and rows there is room for before it
 * will say where any pane goes, so the size of a cell is needed before there
 * is a pane to measure. What is asked is what xterm itself lays its rows out
 * by, drawn the way the panes will be: a figure worked out any other way is
 * a fraction of a pixel off, and across two hundred columns that is a column.
 */
export function openProbe(host: HTMLElement, look: TerminalLook, renderer: TerminalRenderer): Probe {
  const term = new Terminal({
    // Wide, because xterm rounds a row's width to a whole pixel and says a
    // cell is that over the columns: over two columns the rounding is most
    // of a tenth of a pixel a cell, and over two hundred it is nothing.
    cols: 200,
    rows: 1,
    scrollback: 0,
    fontFamily: look.fontFamily,
    fontSize: look.fontSize,
    allowTransparency: true,
  });
  term.open(host);
  let disposed = false;
  if (renderer === "webgl") accelerate(term, () => disposed);

  return {
    cell: () => {
      // Not part of xterm's public face; it is what its own fit addon reads.
      const core = (term as unknown as { _core?: { _renderService?: { dimensions?: Dimensions } } })
        ._core;
      const cell = core?._renderService?.dimensions?.css.cell;
      return cell && cell.width > 0 && cell.height > 0 ? { w: cell.width, h: cell.height } : null;
    },
    restyle: (next) => {
      term.options.fontFamily = next.fontFamily;
      term.options.fontSize = next.fontSize;
    },
    dispose: () => {
      disposed = true;
      term.dispose();
    },
  };
}

type Dimensions = { css: { cell: { width: number; height: number } } };
