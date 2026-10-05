/**
 * The text editor: one CodeMirror view, and a state per open file.
 *
 * One view rather than one per file, swapped with `setState` — a view is the
 * expensive half (DOM, observers), while a state is just the document, its
 * selection and its undo history, which is exactly what has to survive a
 * switch to another file and back.
 *
 * Everything configurable sits in a compartment, so a change to the config
 * re-dresses the files that are already open instead of waiting for the next
 * one. Some of it is the same for every file (the font, the palette) and some
 * is each file's own (its indentation, its language).
 */
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentLess,
  indentMore,
  redo,
  selectAll,
  undo,
} from "@codemirror/commands";
import { HighlightStyle, indentUnit, syntaxHighlighting } from "@codemirror/language";
import {
  Compartment,
  countColumn,
  EditorSelection,
  EditorState,
  type Extension,
  type StateEffect,
  type Text,
} from "@codemirror/state";
import {
  type Command,
  drawSelection,
  EditorView,
  gutter,
  GutterMarker,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";

import type { Indent } from "./indent";

/** 1-based, as a status bar shows it. */
export type Cursor = { line: number; col: number };

export type EditorCommand = "undo" | "redo" | "selectAll";

/** The part of the config that is the same for every file. */
export type EditorLook = {
  /** A CSS font list. */
  fontFamily: string;
  /** Pixels. */
  fontSize: number;
  /** A multiple of the font size. */
  lineHeight: number;
  lineNumbers: boolean;
  /** Number lines by their distance from the cursor. */
  relativeLineNumbers: boolean;
  wordWrap: boolean;
};

type Events = {
  /** A file's text now differs from, or matches again, what was last saved. */
  ondirty: (key: number, dirty: boolean) => void;
  oncursor: (cursor: Cursor) => void;
};

// Colours come from app.css, so the editor follows the window's palette. The
// background stays transparent: the stage behind it carries the opacity.
const chrome = EditorView.theme(
  {
    "&": { height: "100%", color: "var(--fg)", backgroundColor: "transparent" },
    "&.cm-focused": { outline: "none" },
    ".cm-content": { padding: "6px 0", caretColor: "var(--accent)" },
    ".cm-gutters": {
      backgroundColor: "transparent",
      color: "var(--fg-faint)",
      border: "none",
    },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 10px 0 14px" },
    ".cm-activeLine": { backgroundColor: "#ffffff0a" },
    ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--fg-dim)" },
    ".cm-cursor": { borderLeftColor: "var(--accent)" },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground":
      { backgroundColor: "var(--hover-strong)" },
  },
  { dark: true },
);

class NumberMarker extends GutterMarker {
  constructor(readonly text: string) {
    super();
  }
  override eq(other: NumberMarker) {
    return this.text === other.text;
  }
  override toDOM() {
    return document.createTextNode(this.text);
  }
}

/** As many zeros as the last line's number has digits: the width to reserve. */
const widest = (state: EditorState) => "0".repeat(String(state.doc.lines).length);

/**
 * Line numbers counted from the cursor: the cursor's line shows its real
 * number, every other line how far away it is. What a count before a motion
 * (`5j`, `3dd`) needs to be read straight off the gutter.
 *
 * A gutter of its own because the built-in one only renumbers when the text
 * changes, and this has to renumber every time the cursor changes line. It
 * wears the built-in one's class, so it is styled as the same gutter.
 */
const relativeLineNumbers = gutter({
  class: "cm-lineNumbers",
  lineMarker(view, line) {
    const { doc, selection } = view.state;
    const cursor = doc.lineAt(selection.main.head).number;
    const number = doc.lineAt(line.from).number;
    return new NumberMarker(String(number === cursor ? number : Math.abs(number - cursor)));
  },
  lineMarkerChange: (update) => update.selectionSet || update.docChanged,
  initialSpacer: (view) => new NumberMarker(widest(view.state)),
  updateSpacer: (spacer, update) => {
    const width = widest(update.state);
    return (spacer as NumberMarker).text === width ? spacer : new NumberMarker(width);
  },
});

function lookExtension(look: EditorLook): Extension {
  const numbers = look.relativeLineNumbers ? relativeLineNumbers : lineNumbers();
  return [
    EditorView.theme({
      "&": { fontSize: `${look.fontSize}px` },
      ".cm-scroller": { fontFamily: look.fontFamily, lineHeight: String(look.lineHeight) },
    }),
    look.lineNumbers ? [numbers, highlightActiveLineGutter()] : [],
    look.wordWrap ? EditorView.lineWrapping : [],
  ];
}

function indentExtension(indent: Indent): Extension {
  return [
    EditorState.tabSize.of(indent.width),
    indentUnit.of(indent.spaces ? " ".repeat(indent.width) : "\t"),
  ];
}

/**
 * Tab. With something selected it indents the selected lines; otherwise it
 * inserts one step of indentation *at the cursor* — a tab character, or as
 * many spaces as reach the next stop — which is what the key does in every
 * editor people come here from. (CodeMirror's own `indentWithTab` indents the
 * whole line wherever the cursor is.)
 */
const insertIndent: Command = (view) => {
  const { state } = view;
  if (state.readOnly) return false;
  if (state.selection.ranges.some((range) => !range.empty)) return indentMore(view);

  const unit = state.facet(indentUnit);
  view.dispatch(
    state.changeByRange((range) => {
      let insert = "\t";
      if (unit !== "\t") {
        const line = state.doc.lineAt(range.head);
        const column = countColumn(line.text, state.tabSize, range.head - line.from);
        insert = " ".repeat(unit.length - (column % unit.length));
      }
      return {
        changes: { from: range.head, insert },
        range: EditorSelection.cursor(range.head + insert.length),
      };
    }),
    { scrollIntoView: true, userEvent: "input" },
  );
  return true;
};

function cursorOf(state: EditorState): Cursor {
  const head = state.selection.main.head;
  const line = state.doc.lineAt(head);
  return { line: line.number, col: head - line.from + 1 };
}

export class EditorHost {
  private view: EditorView | null = null;
  private states = new Map<number, EditorState>();
  /** Each file's text as of its last save, to tell "modified" from "edited and put back". */
  private saved = new Map<number, Text>();
  private current: number | null = null;

  // Same for every file.
  private readonly look = new Compartment();
  private readonly highlight = new Compartment();
  // Each file's own.
  private readonly indent = new Compartment();
  private readonly language = new Compartment();

  private lookValue: Extension = [];
  /** Empty until the config names a palette; grammars colour nothing before then. */
  private highlightValue: Extension = [];

  /** What the view holds while no file is open: nothing, and not typeable. */
  private readonly blank = EditorState.create({
    extensions: [chrome, EditorView.editable.of(false)],
  });

  constructor(private readonly events: Events) {}

  mount(parent: HTMLElement) {
    this.view = new EditorView({ parent, state: this.stateFor(this.current) });
  }

  unmount() {
    this.stash();
    this.view?.destroy();
    this.view = null;
  }

  /** Start tracking a file. Its text is considered saved as given. */
  create(key: number, text: string, indent: Indent) {
    const state = EditorState.create({
      doc: text,
      extensions: [
        this.look.of(this.lookValue),
        this.indent.of(indentExtension(indent)),
        this.language.of([]),
        this.highlight.of(this.highlightValue),
        highlightActiveLine(),
        history(),
        drawSelection(),
        EditorState.allowMultipleSelections.of(true),
        keymap.of([
          { key: "Tab", run: insertIndent, shift: indentLess },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        chrome,
        EditorView.updateListener.of((update) => {
          const current = this.current;
          if (current === null) return;
          if (update.docChanged) {
            const saved = this.saved.get(current);
            this.events.ondirty(current, !saved || !update.state.doc.eq(saved));
          }
          if (update.docChanged || update.selectionSet) {
            this.events.oncursor(cursorOf(update.state));
          }
        }),
      ],
    });
    this.states.set(key, state);
    this.saved.set(key, state.doc);
  }

  /** Put a file in the view, or `null` for none. */
  show(key: number | null) {
    this.stash();
    this.current = key;
    if (!this.view) return;

    // Not `stateFor`: `current` already names the new file, so that would hand
    // back whatever the view is showing now.
    const state = (key !== null && this.states.get(key)) || this.blank;
    this.view.setState(state);
    // `setState` is not an update, so the listener above never hears of it.
    this.events.oncursor(cursorOf(state));
    if (key !== null) this.view.focus();
  }

  drop(key: number) {
    if (this.current === key) this.current = null;
    this.states.delete(key);
    this.saved.delete(key);
  }

  // --- configuration ----------------------------------------------------------

  /** The font and gutter, for every file. */
  setLook(look: EditorLook) {
    this.lookValue = lookExtension(look);
    this.reconfigureAll(this.look.reconfigure(this.lookValue));
  }

  /** The syntax palette, for every file. */
  setHighlightStyle(style: HighlightStyle) {
    this.highlightValue = syntaxHighlighting(style);
    this.reconfigureAll(this.highlight.reconfigure(this.highlightValue));
  }

  setIndent(key: number, indent: Indent) {
    this.reconfigure(key, this.indent.reconfigure(indentExtension(indent)));
  }

  /** The file's grammar, or `[]` for plain text. */
  setLanguage(key: number, language: Extension) {
    this.reconfigure(key, this.language.reconfigure(language));
  }

  private reconfigureAll(effect: StateEffect<unknown>) {
    for (const key of this.states.keys()) this.reconfigure(key, effect);
  }

  /** Apply an effect to a file, whether it is the one on screen or not. */
  private reconfigure(key: number, effect: StateEffect<unknown>) {
    if (key === this.current && this.view) {
      this.view.dispatch({ effects: effect });
      return;
    }
    const state = this.states.get(key);
    if (state) this.states.set(key, state.update({ effects: effect }).state);
  }

  // --- text -------------------------------------------------------------------

  /** A file's text, `\n`-separated. */
  text(key: number): string {
    return this.stateFor(key).doc.toString();
  }

  markSaved(key: number) {
    this.saved.set(key, this.stateFor(key).doc);
    this.events.ondirty(key, false);
  }

  /** Nothing on disk matches this file any more, whatever its text. */
  markUnsaved(key: number) {
    this.saved.delete(key);
    this.events.ondirty(key, true);
  }

  /**
   * Swap a file's whole text, for when it has changed on disk.
   *
   * An ordinary edit rather than a fresh state, so it lands on the undo stack:
   * Ctrl+Z after a reload brings back what was there before it.
   */
  replace(key: number, text: string) {
    const state = this.states.get(key);
    const live = key === this.current && this.view ? this.view : null;
    const from = live?.state ?? state;
    if (!from) return;

    const doc = from.toText(text);
    const spec = {
      changes: { from: 0, to: from.doc.length, insert: doc },
      // Roughly where it was; the text around it may be entirely different.
      selection: { anchor: Math.min(from.selection.main.head, doc.length) },
    };
    if (live) live.dispatch(spec);
    else this.states.set(key, from.update(spec).state);
  }

  hasFocus(): boolean {
    return this.view?.hasFocus ?? false;
  }

  focus() {
    this.view?.focus();
  }

  run(command: EditorCommand) {
    if (!this.view) return;
    if (command === "undo") undo(this.view);
    else if (command === "redo") redo(this.view);
    else selectAll(this.view);
  }

  /** Replace the selection, as a paste does. */
  insert(text: string) {
    const view = this.view;
    if (!view) return;
    view.dispatch(view.state.replaceSelection(view.state.toText(text)), {
      scrollIntoView: true,
      userEvent: "input.paste",
    });
  }

  /** The live state for the file on screen; the stored one for any other. */
  private stateFor(key: number | null): EditorState {
    if (key === null) return this.blank;
    if (key === this.current && this.view) return this.view.state;
    return this.states.get(key) ?? this.blank;
  }

  /** Keep the on-screen file's state before the view moves on. */
  private stash() {
    if (this.view && this.current !== null) this.states.set(this.current, this.view.state);
  }
}
