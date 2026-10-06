/**
 * The text editor: a CodeMirror view for each pane, and a state for each
 * file a pane has open.
 *
 * One view to a pane rather than one per file, swapped with `setState` — a
 * view is the expensive half (DOM, observers), while a state is just the
 * document, its selection and its undo history, which is exactly what has to
 * survive a switch to another file and back.
 *
 * A file open in two panes is two states of the one text. What is typed into
 * either is passed on to the other as it happens, the way a collaborator's
 * edits would be: each keeps its own cursor and its own scroll, and they
 * never disagree about what the file says.
 *
 * What can be undone belongs to the file, not to a pane, as it does to a
 * buffer in vim: there is one history of each file, kept beside its states
 * and not in any of them, and undo in whichever pane takes back the last
 * thing done to the file, in whichever pane that was.
 *
 * Everything configurable sits in a compartment, so a change to the config
 * re-dresses the files that are already open instead of waiting for the next
 * one. Some of it is the same for every file (the font, the palette) and some
 * is each file's own (its indentation, its language).
 */
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import {
  defaultKeymap,
  history,
  isolateHistory,
  indentLess,
  indentMore,
  insertNewlineAndIndent,
  redo,
  selectAll,
  undo,
} from "@codemirror/commands";
import {
  getIndentation,
  HighlightStyle,
  indentOnInput,
  indentUnit,
  syntaxHighlighting,
} from "@codemirror/language";
import {
  Annotation,
  type ChangeSet,
  Compartment,
  countColumn,
  EditorSelection,
  EditorState,
  type Extension,
  type StateEffect,
  type StateCommand,
  type Text,
  Transaction,
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
  layer,
  type LayerMarker,
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
  /** Glide the caret between positions rather than jumping. */
  smoothCaret: boolean;
  /** Type the closing bracket or quote along with the opening one. */
  autoClose: boolean;
};

type Events = {
  /** A file's text now differs from, or matches again, what was last saved. */
  ondirty: (key: number, dirty: boolean) => void;
  /** A file's text changed, whichever pane it was changed in. */
  onchange: (key: number) => void;
  oncursor: (pane: number, cursor: Cursor) => void;
  /**
   * A pane's view was given a different file, or different extensions. For
   * anything that hangs off the view itself rather than off a file's state.
   */
  onview: (pane: number, view: EditorView) => void;
  /** The keyboard has come to a pane's editor. */
  onfocus: (pane: number) => void;
};

/** Marks an edit that was made in another pane and is being passed on here. */
const passedOn = Annotation.define<boolean>();

/**
 * Where a file's state waits while no pane has it: just opened and not yet
 * shown, or on its way from one pane to another. No pane has this id.
 */
const SPARE = 0;

/** Held, faded out, faded back in. The hold is what a moving caret is seen in. */
const CARET_BLINK = {
  "0%, 32%": { opacity: "1" },
  "66%": { opacity: "0" },
  "100%": { opacity: "1" },
};

// Colours come from app.css, so the editor follows the window's palette. The
// background stays transparent: the stage behind it carries the opacity.
const chrome = EditorView.theme(
  {
    "&": { height: "100%", color: "var(--fg)", backgroundColor: "transparent" },
    "&.cm-focused": { outline: "none" },
    ".cm-content": { padding: "6px 0", caretColor: "var(--caret)" },
    ".cm-gutters": {
      backgroundColor: "transparent",
      color: "var(--fg-faint)",
      border: "none",
      // The numbers are furniture, not text: a drag that starts or strays
      // here must not select them, and Ctrl+A must not sweep them up.
      userSelect: "none",
      WebkitUserSelect: "none",
      cursor: "default",
    },
    // Belt and braces: `user-select` is not inherited by every engine's
    // reckoning, and a number that can be selected can be copied.
    ".cm-gutter, .cm-gutterElement": { userSelect: "none", WebkitUserSelect: "none" },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 10px 0 14px" },
    ".cm-activeLine": { backgroundColor: "#ffffff0a" },
    ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--fg-dim)" },
    // Both stock carets are switched off -- CodeMirror's beam and the vim
    // extension's block live in layers of this class -- in favour of the one
    // caret drawn by `caretLayer` below, which can be either shape.
    ".cm-cursorLayer": { display: "none !important" },
    ".cm-threadCaret": {
      position: "absolute",
      width: "2px",
      borderRadius: "1px",
      backgroundColor: "var(--caret)",
    },
    // The caret breathes: there for a moment, faded out, faded back. Only in
    // the editor being typed into; the rules below for one that is not would
    // lose to an animation.
    //
    // Two names for one set of keyframes. Changing which of them the caret
    // wears is what starts the cycle over each time it moves (see
    // `CaretMarker`), so a caret in motion is always solid and it is only one
    // left alone that starts to fade.
    "@keyframes thread-caret-a": CARET_BLINK,
    "@keyframes thread-caret-b": CARET_BLINK,
    "&.cm-focused .cm-threadCaret": {
      animation: "thread-caret-a 1.25s ease-in-out infinite",
    },
    "&.cm-focused .cm-threadCaret.cm-threadCaret-again": {
      animationName: "thread-caret-b",
    },
    // The vim extension marks the scroller while a block cursor is called
    // for: normal, visual and replace modes. The shape follows from that
    // class alone, so changing mode is a CSS change and the transition on
    // `width` is the whole of the box-to-beam animation.
    // A pixel wider than the character on each side, and rounded: a block
    // exactly the cell's width crowds the letter it is on.
    ".cm-vimMode .cm-threadCaret": {
      width: "calc(var(--caret-cell) + 2px)",
      marginLeft: "-1px",
      borderRadius: "1.5px",
      backgroundColor: "var(--caret-block)",
    },
    // No caret in an editor that is not being typed into; a faint block in
    // vim's modes, where it also marks the place commands will act on.
    "&:not(.cm-focused) .cm-threadCaret": { opacity: "0" },
    "&:not(.cm-focused) .cm-vimMode .cm-threadCaret": { opacity: "0.45" },

    // Vim's command line and messages, in the window's colours and font
    // rather than the extension's own. `!important` where the extension sets
    // the colour inline: its messages are a hard red.
    ".cm-vim-message": { color: "var(--fg-dim) !important" },
    ".cm-panels": {
      backgroundColor: "transparent",
      color: "var(--fg)",
    },
    ".cm-panels-bottom": { borderTop: "1px solid var(--border)" },
    // The command line is shown in a bar of the window's own under the text
    // (see `vim.ts`), which leaves the panel it was made for empty; an empty
    // strip with a rule above it is not worth keeping.
    ".cm-panels:has(.cm-vim-panel:empty)": { display: "none" },
    ".cm-vim-panel": { padding: "3px 14px", fontFamily: "inherit" },
    ".cm-vim-panel input": { color: "var(--fg)", fontFamily: "inherit", fontSize: "inherit" },
    ".cm-searchMatch": { backgroundColor: "var(--hover-strong) !important" },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground":
      { backgroundColor: "var(--selection)" },
  },
  { dark: true },
);

// --- the caret ---------------------------------------------------------------

/**
 * One caret, at one cursor. It carries the position and the width of a
 * character cell; whether it is drawn as a beam or as a block that wide is the
 * stylesheet's business (see `.cm-threadCaret` above).
 */
class CaretMarker implements LayerMarker {
  constructor(
    readonly left: number,
    readonly top: number,
    readonly height: number,
    readonly cell: number,
  ) {}

  eq(other: CaretMarker) {
    return (
      this.left === other.left &&
      this.top === other.top &&
      this.height === other.height &&
      this.cell === other.cell
    );
  }

  draw() {
    const el = document.createElement("div");
    el.className = "cm-threadCaret";
    this.place(el);
    return el;
  }

  // Moved rather than replaced, which is what lets its position and shape be
  // eased from one to the next.
  update(el: HTMLElement) {
    this.place(el);
    // It moved, so its fade starts over from fully there.
    el.classList.toggle("cm-threadCaret-again");
    return true;
  }

  private place(el: HTMLElement) {
    el.style.left = `${this.left}px`;
    el.style.top = `${this.top}px`;
    el.style.height = `${this.height}px`;
    el.style.setProperty("--caret-cell", `${this.cell}px`);
  }
}

/**
 * The caret, drawn by Thread rather than by CodeMirror or the vim extension.
 *
 * Those two each draw their own — a beam and a block — as different elements
 * in different layers, so there is nothing to animate between them: one
 * vanishes and the other appears. A single element that is both is what makes
 * the change of shape a transition.
 *
 * It sits *below* the text. A block can then be a solid colour without hiding
 * the character it is on, and without redrawing that character on top of
 * itself in a matching font, which is how the extension does it.
 */
const caretLayer = layer({
  above: false,
  class: "cm-threadCaretLayer",
  update: (update) =>
    update.docChanged ||
    update.selectionSet ||
    update.geometryChanged ||
    update.viewportChanged ||
    update.focusChanged,
  markers(view) {
    const scroller = view.scrollDOM;
    const block = scroller.classList.contains("cm-vimMode");
    const rect = scroller.getBoundingClientRect();
    const baseLeft = rect.left - scroller.scrollLeft * view.scaleX;
    const baseTop = rect.top - scroller.scrollTop * view.scaleY;

    const carets: CaretMarker[] = [];
    for (const range of view.state.selection.ranges) {
      // In vim's visual mode the selection ends *after* the last character
      // selected, and the block belongs on that character, not past it.
      const at = block && !range.empty && range.head > range.anchor ? range.head - 1 : range.head;
      const coords = view.coordsAtPos(at, 1);
      if (!coords) continue;
      // The full height of the line, not of the letters on it: at a line
      // height of 1.6 a block only as tall as the text is a small box adrift
      // in the row. Centred on the text, so it fills the same row the active
      // line's highlight does -- and measured from the text rather than the
      // line's block, which for a wrapped line is every row of it.
      const height = view.defaultLineHeight;
      const middle = (coords.top + coords.bottom) / 2;
      carets.push(
        new CaretMarker(
          (coords.left - baseLeft) / view.scaleX,
          (middle - baseTop) / view.scaleY - height / 2,
          height,
          view.defaultCharacterWidth,
        ),
      );
    }
    return carets;
  },
});

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
      // The caret is an element that is moved and reshaped, not redrawn, so
      // easing is all either animation takes. The change of shape between
      // beam and block is always eased. Position is eased only for a smooth
      // caret: short, and fast at the start -- long enough to see where it
      // went, not long enough to lag behind typing.
      ".cm-threadCaret": {
        transition: [
          "width 110ms cubic-bezier(0.2, 0.9, 0.3, 1)",
          "margin-left 110ms cubic-bezier(0.2, 0.9, 0.3, 1)",
          "border-radius 110ms ease",
          "background-color 110ms ease",
          "opacity 110ms ease",
          ...(look.smoothCaret
            ? [
                "left 80ms cubic-bezier(0.2, 0.9, 0.3, 1)",
                "top 80ms cubic-bezier(0.2, 0.9, 0.3, 1)",
              ]
            : []),
        ].join(", "),
      },
    }),
    // Ahead of the keymap below it in each file's extensions, which is what
    // lets Backspace between a pair take both of them.
    look.autoClose ? [closeBrackets(), keymap.of(closeBracketsKeymap)] : [],
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

// --- indentation as you type ---------------------------------------------------

const CLOSER: Record<string, string> = { "{": "}", "[": "]", "(": ")" };

/**
 * Enter.
 *
 * Where the file's language can say how a line should be indented, it does:
 * that knows a continued statement from a new one, and a `case` from a brace.
 * Where it cannot (plain text, a language with no grammar here, a grammar
 * still being fetched) the rules are the ones every C-style editor shares:
 *
 * - a new line starts where the one above it did;
 * - one step further in, after an opening bracket;
 * - and between a pair, the pair is opened out, with the cursor on an
 *   indented line of its own and the closing bracket back under the opener:
 *
 *       int main() {
 *           |
 *       }
 */
const newlineAndIndent: Command = (view) => {
  const { state } = view;
  if (state.readOnly) return false;
  if (getIndentation(state, state.selection.main.head) !== null) {
    return insertNewlineAndIndent(view);
  }

  const unit = state.facet(indentUnit);
  view.dispatch(
    state.changeByRange((range) => {
      const line = state.doc.lineAt(range.from);
      const before = line.text.slice(0, range.from - line.from);
      const lead = /^[ \t]*/.exec(before)![0];
      const opener = before.trimEnd().slice(-1);
      const next = state.doc.sliceString(range.to, Math.min(range.to + 1, state.doc.length));

      const inner = opener in CLOSER ? lead + unit : lead;
      const between = opener in CLOSER && next === CLOSER[opener];
      const insert = between ? `\n${inner}\n${lead}` : `\n${inner}`;
      return {
        changes: { from: range.from, to: range.to, insert },
        range: EditorSelection.cursor(range.from + 1 + inner.length),
      };
    }),
    { scrollIntoView: true, userEvent: "input" },
  );
  return true;
};

/**
 * A closing bracket typed on a line of its own steps back out to where its
 * block began. The language does this itself where it can (`indentOnInput`);
 * this is the same thing for where it cannot.
 */
const dedentOnClose = EditorView.inputHandler.of((view, from, to, text) => {
  if (from !== to || !Object.values(CLOSER).includes(text)) return false;
  const { state } = view;
  if (state.readOnly || getIndentation(state, from) !== null) return false;

  const line = state.doc.lineAt(from);
  const before = line.text.slice(0, from - line.from);
  // Only at the start of an otherwise empty stretch of indentation, and not
  // when the bracket is already there to be typed over.
  if (before === "" || before.trim() !== "") return false;
  if (state.doc.sliceString(from, from + 1) === text) return false;

  const unit = state.facet(indentUnit);
  const lead = before.endsWith(unit)
    ? before.slice(0, -unit.length)
    : before.replace(unit === "\t" ? /\t$/ : / {1,8}$/, "");
  view.dispatch({
    changes: { from: line.from, to: from, insert: lead + text },
    selection: EditorSelection.cursor(line.from + lead.length + text.length),
    scrollIntoView: true,
    userEvent: "input.type",
  });
  return true;
});

function cursorOf(state: EditorState): Cursor {
  const head = state.selection.main.head;
  const line = state.doc.lineAt(head);
  return { line: line.number, col: head - line.from + 1 };
}

export class EditorHost {
  /** Each pane's view, and the file it is showing. */
  private panes = new Map<number, { view: EditorView; current: number | null }>();
  /** What each pane should show, asked for before its view existed. */
  private wanted = new Map<number, number | null>();
  /**
   * Every state of every open file, by file and then by pane (or `SPARE`).
   * The entry for a pane that is showing the file is out of date: the view
   * has the live one.
   */
  private states = new Map<number, Map<number, EditorState>>();
  /** Where each pane had each file scrolled to, to put back when it returns. */
  private scrolls = new Map<string, StateEffect<unknown>>();
  /** Each file's text as of its last save, to tell "modified" from "edited and put back". */
  private saved = new Map<number, Text>();

  // Same for every file.
  private readonly vimMode = new Compartment();
  private readonly look = new Compartment();
  private readonly highlight = new Compartment();
  // Each file's own.
  private readonly indent = new Compartment();
  private readonly language = new Compartment();
  private readonly tools = new Compartment();

  private vimValue: Extension = [];
  private lookValue: Extension = [];
  /** Empty until the config names a palette; grammars colour nothing before then. */
  private highlightValue: Extension = [];
  /** What each file's own compartments hold, for dressing another state of it. */
  private own = new Map<number, { indent: Extension; language: Extension; tools: Extension }>();
  /**
   * What can be undone in each file. A state of its own that no pane shows,
   * holding the text and the history of it: every edit made in a pane is
   * made in it too, and undoing is done in it and the result passed out.
   */
  private ledgers = new Map<number, EditorState>();
  /** The last thing its tools were told about each file, for a state made later. */
  private kept = new Map<number, StateEffect<unknown>>();

  /** What a view holds while no file is open: nothing, and not typeable. */
  private readonly blank = EditorState.create({
    extensions: [chrome, EditorView.editable.of(false)],
  });

  constructor(private readonly events: Events) {}

  /** Give a pane its view. */
  mount(pane: number, parent: HTMLElement) {
    const view = new EditorView({ parent, state: this.blank });
    this.panes.set(pane, { view, current: null });
    const wanted = this.wanted.get(pane) ?? null;
    this.wanted.delete(pane);
    if (wanted !== null && this.states.has(wanted)) this.show(pane, wanted);
  }

  unmount(pane: number) {
    const slot = this.panes.get(pane);
    if (!slot) return;
    this.stash(pane);
    slot.view.destroy();
    this.panes.delete(pane);
  }

  /** A fresh state of a file: the text, and a cursor. */
  private make(key: number, doc: Text | string, selection?: EditorSelection): EditorState {
    const kept = this.kept.get(key);
    const state = this.dressed(key, doc, selection);
    return kept ? state.update({ effects: kept }).state : state;
  }

  private dressed(key: number, doc: Text | string, selection?: EditorSelection): EditorState {
    const own = this.own.get(key)!;
    return EditorState.create({
      doc,
      selection,
      extensions: [
        // First, so its key handling is ahead of every keymap below: in
        // normal mode `d` is an operator, not a letter to type.
        this.vimMode.of(this.vimValue),
        this.look.of(this.lookValue),
        this.indent.of(own.indent),
        this.language.of(own.language),
        this.tools.of(own.tools),
        this.highlight.of(this.highlightValue),
        highlightActiveLine(),
        drawSelection(),
        caretLayer,
        EditorState.allowMultipleSelections.of(true),
        indentOnInput(),
        dedentOnClose,
        keymap.of([
          { key: "Tab", run: insertIndent, shift: indentLess },
          { key: "Enter", run: newlineAndIndent },
          // Undo and redo are the file's, not this state's: see `travel`.
          { key: "Mod-z", run: (view) => this.travelIn(view, "undo"), preventDefault: true },
          { key: "Mod-y", run: (view) => this.travelIn(view, "redo"), preventDefault: true },
          { key: "Mod-Shift-z", run: (view) => this.travelIn(view, "redo"), preventDefault: true },
          ...defaultKeymap,
        ]),
        chrome,
        EditorView.updateListener.of((update) => {
          const pane = this.paneOf(update.view);
          if (pane === null) return;
          if (update.docChanged) {
            // What was done here goes to every other state of the file; what
            // arrived from one of them stops here.
            for (const tr of update.transactions) {
              if (!tr.docChanged || tr.annotation(passedOn)) continue;
              this.record(key, tr);
              this.passOn(key, pane, tr.changes);
            }
            const saved = this.saved.get(key);
            this.events.ondirty(key, !saved || !update.state.doc.eq(saved));
            this.events.onchange(key);
          }
          if (update.docChanged || update.selectionSet) {
            this.events.oncursor(pane, cursorOf(update.state));
          }
          if (update.focusChanged && update.view.hasFocus) this.events.onfocus(pane);
        }),
      ],
    });
  }

  /** Make an edit from one pane in every other state of the file. */
  private passOn(key: number, from: number, changes: ChangeSet) {
    const states = this.states.get(key);
    if (!states) return;
    const spec = { changes, annotations: passedOn.of(true) };
    for (const [pane, state] of states) {
      if (pane === from) continue;
      const slot = this.panes.get(pane);
      if (slot?.current === key) slot.view.dispatch(spec);
      else states.set(pane, state.update(spec).state);
    }
  }

  /**
   * Write an edit into the file's history. With what kind of edit it was
   * and when, which is what the history goes by in deciding where one
   * undoable step ends and the next begins.
   */
  private record(key: number, tr: Transaction) {
    const ledger = this.ledgers.get(key);
    if (!ledger) return;
    const carried = [Transaction.userEvent, Transaction.time, Transaction.addToHistory, isolateHistory];
    this.ledgers.set(
      key,
      ledger.update({
        changes: tr.changes,
        annotations: carried.flatMap((type) => {
          const value = tr.annotation(type as typeof Transaction.userEvent);
          return value === undefined ? [] : [(type as typeof Transaction.userEvent).of(value)];
        }),
      }).state,
    );
  }

  /**
   * Undo or redo in a file: take the step in its history, and make the
   * change that amounts to in every state of it. The pane it was asked for
   * in is left with its cursor at the start of what changed, which is where
   * vim leaves it. False if there was nothing to undo, or redo.
   */
  private travel(key: number, pane: number, direction: "undo" | "redo"): boolean {
    const ledger = this.ledgers.get(key);
    const states = this.states.get(key);
    if (!ledger || !states) return false;

    let step: Transaction | null = null;
    const command: StateCommand = direction === "undo" ? undo : redo;
    command({ state: ledger, dispatch: (tr) => (step = tr) });
    const made = step as Transaction | null;
    if (!made) return false;
    this.ledgers.set(key, made.state);
    if (!made.docChanged) return true;

    let start: number | null = null;
    made.changes.iterChangedRanges((_fromA, _toA, fromB) => {
      start ??= fromB;
    });
    for (const [each, state] of states) {
      const here = each === pane && start !== null;
      const spec = {
        changes: made.changes,
        annotations: passedOn.of(true),
        ...(here ? { selection: { anchor: start! }, scrollIntoView: true } : {}),
      };
      const slot = this.panes.get(each);
      if (slot?.current === key) slot.view.dispatch(spec);
      else states.set(each, state.update(spec).state);
    }
    return true;
  }

  /** Undo or redo in whatever file a view is showing. */
  travelIn(view: EditorView, direction: "undo" | "redo"): boolean {
    const pane = this.paneOf(view);
    const key = pane === null ? null : this.panes.get(pane)!.current;
    return pane !== null && key !== null && this.travel(key, pane, direction);
  }

  private paneOf(view: EditorView): number | null {
    for (const [pane, slot] of this.panes) if (slot.view === view) return pane;
    return null;
  }

  /** The file a view is showing, for whatever is handed a view and nothing else. */
  keyOf(view: EditorView): number | null {
    const pane = this.paneOf(view);
    return pane === null ? null : this.panes.get(pane)!.current;
  }

  /** Start tracking a file. Its text is considered saved as given. */
  create(key: number, text: string, indent: Indent) {
    this.own.set(key, { indent: indentExtension(indent), language: [], tools: [] });
    const state = this.make(key, text);
    this.states.set(key, new Map([[SPARE, state]]));
    this.ledgers.set(key, EditorState.create({ doc: state.doc, extensions: history() }));
    this.saved.set(key, state.doc);
  }

  /** Put a file in a pane's view, or `null` for none. */
  show(pane: number, key: number | null) {
    const slot = this.panes.get(pane);
    if (!slot) {
      this.wanted.set(pane, key);
      return;
    }
    if (slot.current === key) return;
    this.stash(pane);
    slot.current = key;

    const state = key === null ? this.blank : this.adopt(pane, key);
    slot.view.setState(state);
    const scroll = key === null ? undefined : this.scrolls.get(`${pane}:${key}`);
    if (scroll) slot.view.dispatch({ effects: scroll });
    // `setState` is not an update, so the listener above never hears of it.
    this.events.oncursor(pane, cursorOf(state));
    this.events.onview(pane, slot.view);
  }

  /**
   * The state a pane shows a file with: its own if it has had the file
   * before, the spare one if there is one waiting, and otherwise a second
   * look at the text another pane has, starting where that pane is.
   */
  private adopt(pane: number, key: number): EditorState {
    const states = this.states.get(key)!;
    const own = states.get(pane);
    if (own) return own;

    let state = states.get(SPARE);
    if (state) {
      states.delete(SPARE);
      const scroll = this.scrolls.get(`${SPARE}:${key}`);
      if (scroll) this.scrolls.set(`${pane}:${key}`, scroll);
      this.scrolls.delete(`${SPARE}:${key}`);
    } else {
      const [other] = states.keys();
      const from = this.live(other, key) ?? states.get(other)!;
      state = this.make(key, from.doc, from.selection);
      const there = this.panes.get(other);
      if (there?.current === key) {
        this.scrolls.set(`${pane}:${key}`, there.view.scrollSnapshot());
      }
    }
    states.set(pane, state);
    return state;
  }

  /**
   * A pane has let go of a file: its tab there was closed, or moved. The
   * state waits as the spare one, so a tab dragged to another pane arrives
   * with its cursor where it was.
   */
  release(pane: number, key: number) {
    const states = this.states.get(key);
    if (!states?.has(pane)) return;
    const slot = this.panes.get(pane);
    if (slot?.current === key) {
      this.stash(pane);
      this.empty(slot);
    }
    states.set(SPARE, states.get(pane)!);
    states.delete(pane);
    const scroll = this.scrolls.get(`${pane}:${key}`);
    if (scroll) this.scrolls.set(`${SPARE}:${key}`, scroll);
    this.scrolls.delete(`${pane}:${key}`);
  }

  /**
   * Take the file out of a view that is showing it. Done here, at once,
   * rather than left for the pane to ask for: a pane with nothing else to
   * show asks for nothing, and would go on showing the file, still editable.
   */
  private empty(slot: { view: EditorView; current: number | null }) {
    slot.current = null;
    slot.view.setState(this.blank);
  }

  /** Stop tracking a file, in every pane. */

  drop(key: number) {
    for (const slot of this.panes.values()) if (slot.current === key) this.empty(slot);
    for (const pane of this.states.get(key)?.keys() ?? []) this.scrolls.delete(`${pane}:${key}`);
    this.states.delete(key);
    this.saved.delete(key);
    this.own.delete(key);
    this.kept.delete(key);
    this.ledgers.delete(key);
  }

  // --- configuration ----------------------------------------------------------

  /** Vim key handling, for every file; `[]` to switch it off. */
  setVim(extension: Extension) {
    this.vimValue = extension;
    this.reconfigureAll(this.vimMode.reconfigure(extension));
    for (const [pane, slot] of this.panes) this.events.onview(pane, slot.view);
  }

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
    const own = this.own.get(key);
    if (!own) return;
    own.indent = indentExtension(indent);
    this.apply(key, this.indent.reconfigure(own.indent));
  }

  /** The file's grammar, or `[]` for plain text. */
  setLanguage(key: number, language: Extension) {
    const own = this.own.get(key);
    if (!own) return;
    own.language = language;
    this.apply(key, this.language.reconfigure(language));
  }

  /** What a language server adds to a file: completion, and its complaints. */
  setTools(key: number, tools: Extension) {
    const own = this.own.get(key);
    if (!own) return;
    own.tools = tools;
    // Whatever the last tools were told is not something these were.
    this.kept.delete(key);
    this.apply(key, this.tools.reconfigure(tools));
  }

  /**
   * Apply an effect to every state of a file, and to any made from now on:
   * a second pane opened on the file is shown what the first already is.
   * For what a file's tools are told, each telling replacing the last.
   */
  keep(key: number, effect: StateEffect<unknown>) {
    if (!this.states.has(key)) return;
    this.kept.set(key, effect);
    this.apply(key, effect);
  }


  private reconfigureAll(effect: StateEffect<unknown>) {
    for (const key of this.states.keys()) this.apply(key, effect);
  }

  /** Apply an effect to every state of a file, on screen or not. */
  apply(key: number, effect: StateEffect<unknown>) {
    const states = this.states.get(key);
    if (!states) return;
    for (const [pane, state] of states) {
      const slot = this.panes.get(pane);
      if (slot?.current === key) slot.view.dispatch({ effects: effect });
      else states.set(pane, state.update({ effects: effect }).state);
    }
  }

  // --- text -------------------------------------------------------------------

  /** A file's state where it is on screen in `pane`, which is the only live one. */
  private live(pane: number, key: number): EditorState | null {
    const slot = this.panes.get(pane);
    return slot?.current === key ? slot.view.state : null;
  }

  /** Any state of a file: they all hold the same text. */
  private stateOf(key: number): EditorState | null {
    const states = this.states.get(key);
    if (!states) return null;
    for (const [pane, state] of states) return this.live(pane, key) ?? state;
    return null;
  }

  /** A file's text, `\n`-separated. */
  text(key: number): string {
    return this.stateOf(key)?.doc.toString() ?? "";
  }

  /** A file's text as the editor holds it, for turning lines into offsets. */
  doc(key: number): Text | null {
    return this.stateOf(key)?.doc ?? null;
  }

  markSaved(key: number) {
    const state = this.stateOf(key);
    if (state) this.saved.set(key, state.doc);
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
    const states = this.states.get(key);
    if (!states) return;
    // Made in one state, on screen for preference, and passed on to the rest
    // as any edit is.
    const panes = [...states.keys()];
    const pane = panes.find((each) => this.live(each, key) !== null) ?? panes[0];
    const from = this.live(pane, key) ?? states.get(pane)!;

    const doc = from.toText(text);
    const spec = {
      changes: { from: 0, to: from.doc.length, insert: doc },
      // Roughly where it was; the text around it may be entirely different.
      selection: { anchor: Math.min(from.selection.main.head, doc.length) },
    };
    if (this.live(pane, key)) {
      this.panes.get(pane)!.view.dispatch(spec);
      return;
    }
    const tr = from.update(spec);
    states.set(pane, tr.state);
    this.record(key, tr);
    this.passOn(key, pane, tr.changes);
    this.events.onchange(key);
  }

  /** The view the keyboard is in, if it is in one. */
  private focusedView(): EditorView | null {
    for (const slot of this.panes.values()) if (slot.view.hasFocus) return slot.view;
    return null;
  }

  hasFocus(): boolean {
    return this.focusedView() !== null;
  }

  focus(pane: number) {
    this.panes.get(pane)?.view.focus();
  }

  run(command: EditorCommand) {
    const view = this.focusedView();
    if (!view) return;
    if (command === "selectAll") selectAll(view);
    else this.travelIn(view, command);
  }

  /** Replace the selection, as a paste does. */
  insert(text: string) {
    const view = this.focusedView();
    if (!view) return;
    view.dispatch(view.state.replaceSelection(view.state.toText(text)), {
      scrollIntoView: true,
      userEvent: "input.paste",
    });
  }

  /** Keep what a pane is showing, and where, before its view moves on. */
  private stash(pane: number) {
    const slot = this.panes.get(pane);
    if (!slot || slot.current === null) return;
    const states = this.states.get(slot.current);
    if (!states?.has(pane)) return;
    states.set(pane, slot.view.state);
    this.scrolls.set(`${pane}:${slot.current}`, slot.view.scrollSnapshot());
  }
}
