/**
 * The text editor: one CodeMirror view, and a state per open file.
 *
 * One view rather than one per file, swapped with `setState` — a view is the
 * expensive half (DOM, observers), while a state is just the document, its
 * selection and its undo history, which is exactly what has to survive a
 * switch to another file and back.
 *
 * Plain text for now: no language, no highlighting, no completion.
 */
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
  redo,
  selectAll,
  undo,
} from "@codemirror/commands";
import { EditorState, type Extension, type Text } from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view";

/** 1-based, as a status bar shows it. */
export type Cursor = { line: number; col: number };

export type EditorCommand = "undo" | "redo" | "selectAll";

type Events = {
  /** A file's text now differs from, or matches again, what was last saved. */
  ondirty: (key: number, dirty: boolean) => void;
  oncursor: (cursor: Cursor) => void;
};

// Colours come from app.css, so the editor follows the window's palette. The
// background stays transparent: the stage behind it carries the opacity.
const theme = EditorView.theme(
  {
    "&": {
      height: "100%",
      color: "var(--fg)",
      backgroundColor: "transparent",
      fontSize: "13px",
    },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": {
      fontFamily: '"Cascadia Mono", Consolas, "Courier New", monospace',
      lineHeight: "1.5",
    },
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

  private readonly extensions: Extension;
  /** What the view holds while no file is open: nothing, and not typeable. */
  private readonly blank = EditorState.create({ extensions: [theme, EditorView.editable.of(false)] });

  constructor(private readonly events: Events) {
    this.extensions = [
      lineNumbers(),
      highlightActiveLine(),
      highlightActiveLineGutter(),
      history(),
      drawSelection(),
      EditorState.allowMultipleSelections.of(true),
      EditorState.tabSize.of(4),
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
      theme,
      EditorView.updateListener.of((update) => {
        const key = this.current;
        if (key === null) return;
        if (update.docChanged) {
          const saved = this.saved.get(key);
          this.events.ondirty(key, !saved || !update.state.doc.eq(saved));
        }
        if (update.docChanged || update.selectionSet) this.events.oncursor(cursorOf(update.state));
      }),
    ];
  }

  mount(parent: HTMLElement) {
    this.view = new EditorView({ parent, state: this.stateFor(this.current) });
  }

  unmount() {
    this.stash();
    this.view?.destroy();
    this.view = null;
  }

  /** Start tracking a file. Its text is considered saved as given. */
  create(key: number, text: string) {
    const state = EditorState.create({ doc: text, extensions: this.extensions });
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

  /** A file's text, `\n`-separated. */
  text(key: number): string {
    return this.stateFor(key).doc.toString();
  }

  markSaved(key: number) {
    this.saved.set(key, this.stateFor(key).doc);
    this.events.ondirty(key, false);
  }

  hasFocus(): boolean {
    return this.view?.hasFocus ?? false;
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
