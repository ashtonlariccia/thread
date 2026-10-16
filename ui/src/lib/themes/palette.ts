/**
 * What a colour theme is made of, and how the parts of the window are made
 * from it.
 *
 * A palette is one variant of a theme — its dark or its light — as a flat set
 * of colours with a job each. The syntax style, the window's CSS variables and
 * the terminal's sixteen colours are all derived from it here, so a theme
 * file is only colours, and every theme highlights the same things the same
 * way.
 */
import { HighlightStyle } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

export type Mode = "dark" | "light";

export type Palette = {
  /** The window's frame, as the three numbers of an `rgb()`: it is drawn at the opacity setting. */
  chrome: string;
  /** What is laid over the frame to make the viewport: darker in the dark, lighter in the light. */
  wash: string;
  /** Opaque surfaces: menus and dialogs, and text fields. */
  menu: string;
  input: string;
  border: string;
  borderInput: string;
  fg: string;
  fgDim: string;
  fgFaint: string;
  caret: string;
  /** The block caret, which sits behind the character it is on. */
  caretBlock: string;
  selection: string;
  danger: string;
  ok: string;

  /** Code, by what each colour is given to. */
  syntax: {
    keyword: string;
    string: string;
    /** Numbers and constants, regexps, attributes. */
    constant: string;
    /** Type names and labels. */
    type: string;
    func: string;
    /** Built-in types and macros. */
    builtin: string;
    property: string;
    /** Operators and escapes. */
    operator: string;
    /** The preprocessor's own language. */
    special: string;
    /** `self`, and what is wrong. */
    self: string;
    comment: string;
    punctuation: string;
  };

  /** The terminal's colours. The bright ones are the same, but for these two. */
  ansi: {
    black: string;
    red: string;
    green: string;
    yellow: string;
    blue: string;
    magenta: string;
    cyan: string;
    white: string;
    /** What a shell dims its suggestions with, so it has to stay readable. */
    brightBlack: string;
    brightWhite: string;
  };

  /** Starting points for the accent; the first is the theme's own. */
  accents: { name: string; color: string }[];
};

export type ColorTheme = {
  /** As the config names it. */
  name: string;
  /** As the dialog shows it. */
  label: string;
  dark: Palette;
  light: Palette;
};

/**
 * Code's colours from a palette. The *font styles* — what is italic and what
 * is bold — are the same for every theme: Monokai Pro's, which italicises the
 * words that declare things (`fn`, `let`, `pub`), primitive types, macros and
 * attributes, and leaves function and type names upright.
 */
export function highlightStyle(p: Palette): HighlightStyle {
  const s = p.syntax;
  return HighlightStyle.define([
    { tag: t.comment, color: s.comment, fontStyle: "italic" },

    { tag: [t.string, t.special(t.string)], color: s.string },
    { tag: t.docString, color: s.string, fontStyle: "italic" },
    { tag: t.regexp, color: s.constant },
    { tag: t.escape, color: s.operator },

    { tag: [t.number, t.bool, t.null, t.atom, t.constant(t.variableName)], color: s.constant },

    { tag: [t.keyword, t.operatorKeyword], color: s.keyword },
    // The words that declare and qualify: Monokai's `storage` scopes.
    { tag: [t.definitionKeyword, t.modifier], color: s.keyword, fontStyle: "italic" },
    { tag: t.operator, color: s.operator },

    { tag: t.standard(t.typeName), color: s.builtin, fontStyle: "italic" },
    { tag: [t.typeName, t.className, t.namespace], color: s.type },

    { tag: [t.function(t.variableName), t.function(t.propertyName)], color: s.func },
    { tag: [t.macroName, t.standard(t.function(t.variableName))], color: s.builtin, fontStyle: "italic" },

    { tag: t.propertyName, color: s.property },
    { tag: t.self, color: s.self, fontStyle: "italic" },
    { tag: t.labelName, color: s.type },

    // Markup: the element, then what is said about it.
    { tag: t.tagName, color: s.keyword },
    { tag: t.attributeName, color: s.constant, fontStyle: "italic" },

    // Decorators and attributes, and the preprocessor's own little language.
    { tag: [t.meta, t.annotation], color: s.constant, fontStyle: "italic" },
    { tag: t.processingInstruction, color: s.special },

    { tag: [t.punctuation, t.bracket, t.separator], color: s.punctuation },

    { tag: t.invalid, color: s.self },
    { tag: t.deleted, color: s.self },
    { tag: t.inserted, color: s.string },
    { tag: t.changed, color: s.type },

    // Prose.
    { tag: t.heading, color: s.keyword, fontWeight: "bold" },
    { tag: t.strong, color: s.constant, fontWeight: "bold" },
    { tag: t.emphasis, color: s.type, fontStyle: "italic" },
    { tag: t.strikethrough, textDecoration: "line-through" },
    { tag: [t.link, t.url], color: s.func },
    { tag: t.quote, color: s.string, fontStyle: "italic" },
    { tag: t.monospace, color: s.operator },
  ]);
}

/** The CSS variables `app.css` names, as a palette sets them. */
export function windowVars(p: Palette, mode: Mode): Record<string, string> {
  const a = p.ansi;
  const ansi = [a.black, a.red, a.green, a.yellow, a.blue, a.magenta, a.cyan, a.white];
  const bright = [a.brightBlack, ...ansi.slice(1, 7), a.brightWhite];
  // Washes over whatever is under them: white in the dark, black in the light.
  const ink = mode === "dark" ? "255 255 255" : "0 0 0";
  return {
    "--chrome-rgb": p.chrome,
    "--bg-viewport-wash": p.wash,
    "--bg-menu": p.menu,
    "--bg-input": p.input,
    "--border": p.border,
    "--border-input": p.borderInput,
    "--fg": p.fg,
    "--fg-dim": p.fgDim,
    "--fg-faint": p.fgFaint,
    "--caret": p.caret,
    "--caret-block": p.caretBlock,
    "--selection": p.selection,
    "--danger": p.danger,
    "--ok": p.ok,
    "--hover": mode === "dark" ? "#ffffff16" : "#0000000f",
    "--active-line": `rgb(${ink} / ${mode === "dark" ? 0.04 : 0.035})`,
    "--guide": `rgb(${ink} / 0.08)`,
    "--field-wash": `rgb(${ink} / 0.07)`,
    "--field-wash-border": `rgb(${ink} / 0.12)`,
    "--preview-match": `color-mix(in srgb, ${p.danger} 24%, transparent)`,
    "--preview-replace": `color-mix(in srgb, ${p.ok} 25%, transparent)`,
    ...Object.fromEntries([...ansi, ...bright].map((color, i) => [`--ansi-${i}`, color])),
  };
}
