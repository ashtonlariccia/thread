/** Solarized: the same accents on either of its two grounds. */
import type { ColorTheme } from "./palette";

const syntax = {
  keyword: "#859900",
  string: "#2aa198",
  constant: "#d33682",
  type: "#b58900",
  func: "#268bd2",
  builtin: "#cb4b16",
  property: "#6c71c4",
  special: "#6c71c4",
  self: "#dc322f",
};

const ansi = {
  red: "#dc322f",
  green: "#859900",
  yellow: "#b58900",
  blue: "#268bd2",
  magenta: "#d33682",
  cyan: "#2aa198",
};

const accents = [
  { name: "Blue", color: "#268bd2" },
  { name: "Cyan", color: "#2aa198" },
  { name: "Green", color: "#859900" },
  { name: "Yellow", color: "#b58900" },
  { name: "Orange", color: "#cb4b16" },
  { name: "Red", color: "#dc322f" },
  { name: "Magenta", color: "#d33682" },
  { name: "Violet", color: "#6c71c4" },
];

export const solarized: ColorTheme = {
  name: "solarized",
  label: "Solarized",
  dark: {
    chrome: "7 54 66",
    wash: "rgb(0 0 0 / 0.2)",
    menu: "#073642",
    input: "#002b36",
    border: "#0e3f4c",
    borderInput: "#1f4d59",
    fg: "#93a1a1",
    fgDim: "#839496",
    fgFaint: "#586e75",
    caret: "#93a1a1",
    caretBlock: "#2a535e",
    selection: "#174652",
    danger: "#dc322f",
    ok: "#859900",
    syntax: { ...syntax, operator: "#93a1a1", comment: "#586e75", punctuation: "#839496" },
    ansi: { ...ansi, black: "#2f5560", white: "#839496", brightBlack: "#657b83", brightWhite: "#eee8d5" },
    accents,
  },
  light: {
    chrome: "238 232 213",
    wash: "rgb(253 246 227 / 0.8)",
    menu: "#fdf6e3",
    input: "#fdf6e3",
    border: "#ddd6c1",
    borderInput: "#c9c2ae",
    fg: "#586e75",
    fgDim: "#657b83",
    fgFaint: "#93a1a1",
    caret: "#586e75",
    caretBlock: "#ddd6c1",
    selection: "#e6dfca",
    danger: "#dc322f",
    ok: "#859900",
    syntax: { ...syntax, operator: "#586e75", comment: "#93a1a1", punctuation: "#657b83" },
    ansi: { ...ansi, black: "#073642", white: "#93a1a1", brightBlack: "#657b83", brightWhite: "#c9c2ae" },
    accents,
  },
};
