/**
 * The colour themes `[theme] syntax` in the config can name, each in a dark
 * and a light variant that `[theme] mode` picks between.
 *
 * A theme colours the whole window: its frame and surfaces, the code, and the
 * terminal. Only the accent is separate, being a setting of its own. Adding a
 * theme is a file beside this, exporting a `ColorTheme`, and a line in the
 * list below.
 */
import type { HighlightStyle } from "@codemirror/language";

import { catppuccin } from "./catppuccin";
import { gruvbox } from "./gruvbox";
import { one } from "./one";
import { highlightStyle, windowVars, type ColorTheme, type Palette } from "./palette";
import { rosePine } from "./rosePine";
import { solarized } from "./solarized";
import { tokyoNight } from "./tokyoNight";

export type { Mode, Palette } from "./palette";

export const DEFAULT_THEME = "catppuccin";

export const THEMES: ColorTheme[] = [catppuccin, gruvbox, tokyoNight, rosePine, one, solarized];

export const THEME_NAMES = THEMES.map((theme) => theme.name);

/**
 * The theme a name means. A name that is not known gets the default: a typo
 * in the config should cost the theme, not the colours.
 */
function themeOf(name: string): ColorTheme {
  const wanted = name.trim().toLowerCase();
  return THEMES.find((theme) => theme.name === wanted) ?? catppuccin;
}

/** One variant of a theme. Anything but "light" is the dark. */
export function paletteOf(name: string, mode: string = "dark"): Palette {
  const theme = themeOf(name);
  return mode === "light" ? theme.light : theme.dark;
}

/** Made once each: an editor reconfigures only when the style is a new one. */
const styles = new Map<Palette, HighlightStyle>();

/** The code's colours, for a theme name and a mode. */
export function syntaxTheme(name: string, mode: string = "dark"): HighlightStyle {
  const palette = paletteOf(name, mode);
  let style = styles.get(palette);
  if (!style) {
    style = highlightStyle(palette);
    styles.set(palette, style);
  }
  return style;
}

/** Put a theme into effect across the window: every surface, and what the browser draws itself. */
export function applyTheme(name: string, mode: string) {
  const light = mode === "light";
  const root = document.documentElement;
  for (const [key, value] of Object.entries(windowVars(paletteOf(name, mode), light ? "light" : "dark"))) {
    root.style.setProperty(key, value);
  }
  root.style.colorScheme = light ? "light" : "dark";
  root.dataset.mode = light ? "light" : "dark";
  // For what reads its colours once rather than from CSS: the terminals.
  window.dispatchEvent(new Event(THEME_EVENT));
}

/** Sent on the window whenever a theme is put into effect. */
export const THEME_EVENT = "tide:theme";

/**
 * The accent to go with a change of theme or mode. One of the old variant's
 * own accents becomes the one in the same place in the new — Mocha's mauve
 * becomes Latte's, Gruvbox's orange becomes its light orange — and a colour
 * of the user's own is kept as it is.
 */
export function carryAccent(accent: string, from: Palette, to: Palette): string {
  const at = from.accents.findIndex((preset) => preset.color === accent.toLowerCase());
  if (at < 0) return accent;
  return (to.accents[at] ?? to.accents[0]).color;
}
