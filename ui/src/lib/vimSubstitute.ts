/**
 * A live preview of `:s`, as Neovim's `inccommand` gives.
 *
 * The vim extension runs a substitute when Enter is pressed and shows nothing
 * before that. This watches what is being typed on its `:` line and, while it
 * reads as a substitute command, shows the result in the text: matches are
 * highlighted as soon as there is a pattern, and shown *as their replacement*
 * once there is one. Nothing is changed in the buffer until the command is
 * actually run; Escape leaves no trace.
 *
 * It keys off the extension's DOM (the `.cm-vim-panel` prompt), not its
 * internals, so it does not need the extension loaded to be imported.
 */
import { type Extension, StateEffect, StateField, type Text } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, WidgetType } from "@codemirror/view";

export type SubstituteRange =
  | { kind: "current" }
  | { kind: "all" }
  | { kind: "selection" }
  /** 1-based, inclusive. `to` may be Infinity for `$`. */
  | { kind: "lines"; from: number; to: number };

export type Substitute = {
  range: SubstituteRange;
  pattern: string;
  /** Null until the second delimiter has been typed. */
  replacement: string | null;
  /** Every match on a line, not just the first. */
  global: boolean;
  ignoreCase: boolean;
};

/** A match and what it would become; `insert` is null when only highlighting. */
export type PreviewEdit = { from: number; to: number; insert: string | null };

/** One end of a range: a line number, `.`, or `$`. */
function address(token: string, current: number): number | null {
  if (token === ".") return current;
  if (token === "$") return Infinity;
  return /^\d+$/.test(token) ? Number(token) : null;
}

/** Split on a delimiter that is not escaped, keeping the escapes. */
function splitUnescaped(text: string, delimiter: string): string[] {
  const parts: string[] = [];
  let part = "";
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "\\" && i + 1 < text.length) {
      part += text[i] + text[i + 1];
      i++;
    } else if (text[i] === delimiter) {
      parts.push(part);
      part = "";
    } else {
      part += text[i];
    }
  }
  parts.push(part);
  return parts;
}

/**
 * Read what is on the `:` line as a substitute command, as far as it has been
 * typed. Null for anything else — another command, or one not far enough along
 * to have a pattern.
 */
export function parseSubstitute(input: string, currentLine = 1): Substitute | null {
  const match = /^\s*(%|'<,'>|[\d.$]+(?:,[\d.$]+)?)?\s*s(?:u(?:b(?:s(?:t(?:i(?:t(?:u(?:te?)?)?)?)?)?)?)?)?([^\w\s\\"|])(.*)$/s.exec(
    input,
  );
  if (!match) return null;
  const [, where, delimiter, rest] = match;

  let range: SubstituteRange;
  if (!where) range = { kind: "current" };
  else if (where === "%") range = { kind: "all" };
  else if (where === "'<,'>") range = { kind: "selection" };
  else {
    const [a, b = a] = where.split(",");
    const from = address(a, currentLine);
    const to = address(b, currentLine);
    if (from === null || to === null) return null;
    range = { kind: "lines", from: Math.min(from, to), to: Math.max(from, to) };
  }

  const [pattern, replacement, flags = ""] = splitUnescaped(rest, delimiter);
  if (!pattern) return null;

  return {
    range,
    pattern,
    replacement: replacement ?? null,
    global: flags.includes("g"),
    ignoreCase: flags.includes("i"),
  };
}

/** Expand a replacement for one match: `&`, `$1`/`\1`, `\n`, `\t`, `\x`. */
function expand(replacement: string, match: RegExpExecArray): string {
  return replacement.replace(/\\(.)|\$(\d)|&/g, (whole, escaped: string | undefined, group: string | undefined) => {
    if (whole === "&") return match[0];
    if (group !== undefined) return match[Number(group)] ?? "";
    if (escaped === "n") return "\n";
    if (escaped === "t") return "\t";
    if (escaped !== undefined && /\d/.test(escaped)) return match[Number(escaped)] ?? "";
    return escaped ?? whole;
  });
}

/**
 * What a substitute would do to the lines `fromLine..toLine` of a document
 * (1-based, inclusive), without doing it.
 *
 * The pattern is a JavaScript regular expression, which is what the vim
 * extension itself uses. One that does not compile — as most do, part-way
 * through being typed — simply previews nothing.
 */
export function previewSubstitute(
  doc: Text,
  command: Substitute,
  fromLine: number,
  toLine: number,
  limit = 2000,
): PreviewEdit[] {
  let regex: RegExp;
  try {
    regex = new RegExp(command.pattern, command.ignoreCase ? "gi" : "g");
  } catch {
    return [];
  }

  const edits: PreviewEdit[] = [];
  const last = Math.min(toLine, doc.lines);
  for (let number = Math.max(1, fromLine); number <= last && edits.length < limit; number++) {
    const line = doc.line(number);
    regex.lastIndex = 0;
    for (let match = regex.exec(line.text); match; match = regex.exec(line.text)) {
      // An empty match would be found again at the same place for ever.
      if (match[0] === "") {
        regex.lastIndex++;
        continue;
      }
      edits.push({
        from: line.from + match.index,
        to: line.from + match.index + match[0].length,
        insert: command.replacement === null ? null : expand(command.replacement, match),
      });
      if (!command.global || edits.length >= limit) break;
    }
  }
  return edits;
}

// --- the editor extension -----------------------------------------------------

class ReplacementWidget extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  override eq(other: ReplacementWidget) {
    return this.text === other.text;
  }
  override toDOM() {
    const span = document.createElement("span");
    span.className = "cm-substitutePreview";
    // A replacement can put in a line break; the preview stays on one line.
    span.textContent = this.text.replaceAll("\n", "⏎");
    return span;
  }
}

const setPreview = StateEffect.define<PreviewEdit[]>();

const previewField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(preview, tr) {
    for (const effect of tr.effects) {
      if (!effect.is(setPreview)) continue;
      return Decoration.set(
        effect.value.map(({ from, to, insert }) =>
          insert === null
            ? Decoration.mark({ class: "cm-substituteMatch" }).range(from, to)
            : Decoration.replace({ widget: new ReplacementWidget(insert) }).range(from, to),
        ),
      );
    }
    // The command ran, or the text changed some other way: the preview was
    // of text that is no longer there.
    return tr.docChanged ? Decoration.none : preview;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** How far outside what is on screen to preview, so a small scroll is covered. */
const MARGIN_LINES = 200;

const watcher = ViewPlugin.fromClass(
  class {
    /** The lines of the last visual selection, for a `'<,'>` range. */
    private selected: { from: number; to: number } | null = null;
    private showing = false;

    constructor(private readonly view: EditorView) {
      const dom = view.dom;
      dom.addEventListener("input", this.onInput, true);
      dom.addEventListener("keydown", this.onKeydown, true);
      dom.addEventListener("focusout", this.onFocusOut, true);
    }

    update() {
      // `:` leaves visual mode, so by the time the command is being typed the
      // selection it refers to is gone. Remember it while it is there.
      const { doc, selection } = this.view.state;
      const range = selection.main;
      if (!range.empty) {
        this.selected = { from: doc.lineAt(range.from).number, to: doc.lineAt(range.to).number };
      }
    }

    destroy() {
      const dom = this.view.dom;
      dom.removeEventListener("input", this.onInput, true);
      dom.removeEventListener("keydown", this.onKeydown, true);
      dom.removeEventListener("focusout", this.onFocusOut, true);
    }

    /** The prompt's input, if this event is from vim's `:` line. */
    private exInput(event: Event): HTMLInputElement | null {
      const target = event.target;
      if (!(target instanceof HTMLInputElement)) return null;
      const panel = target.closest(".cm-vim-panel");
      // `/` and `?` searches use the same prompt, and preview themselves.
      return panel?.textContent?.trimStart().startsWith(":") ? target : null;
    }

    private onInput = (event: Event) => {
      const input = this.exInput(event);
      if (input) this.show(input.value);
    };

    // Before the extension's own handler runs the command: what is on screen
    // must be the real text again by the time it is changed.
    private onKeydown = (event: KeyboardEvent) => {
      if ((event.key === "Enter" || event.key === "Escape") && this.exInput(event)) this.clear();
    };

    private onFocusOut = (event: Event) => {
      if (this.exInput(event)) this.clear();
    };

    private show(typed: string) {
      const { state } = this.view;
      const cursor = state.doc.lineAt(state.selection.main.head).number;
      const command = parseSubstitute(typed, cursor);
      if (!command) return this.clear();

      let from = cursor;
      let to = cursor;
      const range = command.range;
      if (range.kind === "all") [from, to] = [1, state.doc.lines];
      else if (range.kind === "lines") [from, to] = [range.from, range.to];
      else if (range.kind === "selection") {
        if (!this.selected) return this.clear();
        ({ from, to } = this.selected);
      }

      // Only around what can be seen: a whole-file preview of a large file is
      // work nobody is looking at.
      const { from: top, to: bottom } = this.view.viewport;
      from = Math.max(from, state.doc.lineAt(top).number - MARGIN_LINES);
      to = Math.min(to, state.doc.lineAt(bottom).number + MARGIN_LINES);

      this.showing = true;
      this.view.dispatch({ effects: setPreview.of(previewSubstitute(state.doc, command, from, to)) });
    }

    private clear() {
      if (!this.showing) return;
      this.showing = false;
      this.view.dispatch({ effects: setPreview.of([]) });
    }
  },
);

const previewTheme = EditorView.baseTheme({
  ".cm-substituteMatch": { backgroundColor: "var(--preview-match)", borderRadius: "2px" },
  ".cm-substitutePreview": {
    backgroundColor: "var(--preview-replace)",
    color: "var(--fg)",
    borderRadius: "2px",
  },
});

export function substitutePreview(): Extension {
  return [previewField, watcher, previewTheme];
}
