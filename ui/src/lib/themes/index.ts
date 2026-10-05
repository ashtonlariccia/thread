/**
 * The syntax palettes `[theme] syntax` in the config can name.
 *
 * A theme here is only the colours of highlighted code — the window's own
 * colours are not part of it. Adding one is a file beside this, exporting a
 * `HighlightStyle`, and a line in the table below.
 */
import type { HighlightStyle } from "@codemirror/language";

import { catppuccin } from "./catppuccin";

export const DEFAULT_THEME = "catppuccin";

const THEMES: Record<string, HighlightStyle> = { catppuccin };

export const THEME_NAMES = Object.keys(THEMES);

/**
 * The palette for a theme name. A name that is not known gets the default:
 * a typo in the config should cost the theme, not the highlighting.
 */
export function syntaxTheme(name: string): HighlightStyle {
  return THEMES[name.trim().toLowerCase()] ?? THEMES[DEFAULT_THEME];
}
