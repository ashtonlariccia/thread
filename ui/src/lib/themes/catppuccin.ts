/**
 * Catppuccin, as a syntax palette.
 *
 * The colours and the italics are the ones the Catppuccin Charcoal VS Code
 * theme gives each kind of token, so a file reads the same here as it does
 * there. Only the mapping differs: VS Code themes address TextMate scopes,
 * CodeMirror addresses the tags below, and the two do not line up one to one.
 * Where a scope has no tag (a function's parameters, Rust lifetimes) the token
 * falls back to the nearest thing that does.
 */
import { HighlightStyle } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

const mauve = "#cba6f7";
const red = "#f38ba8";
const peach = "#fab387";
const yellow = "#f9e2af";
const green = "#a6e3a1";
const teal = "#94e2d5";
const sky = "#89dceb";
const sapphire = "#74c7ec";
const blue = "#89b4fa";
const lavender = "#b4befe";
const comment = "#7a7a7a";
const punctuation = "#a0a0a0";

export const catppuccin = HighlightStyle.define([
  { tag: t.comment, color: comment, fontStyle: "italic" },

  { tag: [t.string, t.special(t.string)], color: green },
  { tag: t.docString, color: green, fontStyle: "italic" },
  { tag: t.regexp, color: peach },
  { tag: t.escape, color: teal },

  { tag: [t.number, t.bool, t.null, t.atom, t.constant(t.variableName)], color: peach },

  { tag: [t.keyword, t.modifier, t.operatorKeyword], color: mauve },
  { tag: t.operator, color: teal },

  { tag: t.standard(t.typeName), color: sapphire },
  { tag: [t.typeName, t.className, t.namespace], color: yellow, fontStyle: "italic" },

  {
    tag: [t.function(t.variableName), t.function(t.propertyName)],
    color: blue,
    fontStyle: "italic",
  },
  { tag: [t.macroName, t.standard(t.function(t.variableName))], color: sapphire, fontStyle: "italic" },

  { tag: t.propertyName, color: lavender },
  { tag: t.self, color: red, fontStyle: "italic" },
  { tag: t.labelName, color: yellow },

  // Markup: the element, then what is said about it.
  { tag: t.tagName, color: mauve },
  { tag: t.attributeName, color: peach },

  // Decorators and attributes, and the preprocessor's own little language.
  { tag: [t.meta, t.annotation], color: peach },
  { tag: t.processingInstruction, color: sky },

  { tag: [t.punctuation, t.bracket, t.separator], color: punctuation },

  { tag: t.invalid, color: red },
  { tag: t.deleted, color: red },
  { tag: t.inserted, color: green },
  { tag: t.changed, color: yellow },

  // Prose.
  { tag: t.heading, color: mauve, fontWeight: "bold" },
  { tag: t.strong, color: peach, fontWeight: "bold" },
  { tag: t.emphasis, color: yellow, fontStyle: "italic" },
  { tag: t.strikethrough, textDecoration: "line-through" },
  { tag: [t.link, t.url], color: blue },
  { tag: t.quote, color: green, fontStyle: "italic" },
  { tag: t.monospace, color: teal },
]);
