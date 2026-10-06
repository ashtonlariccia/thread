/**
 * The accent colour: the one colour the window highlights things with.
 *
 * `app.css` names four variables that are all the accent in some form: the
 * colour itself, two washes of it, and the ink for text set on top of it.
 * They are derived here from the one colour in the config, so changing it
 * moves every one of them together.
 */

/** Catppuccin's accents, as starting points; any colour can be chosen. */
export const ACCENT_PRESETS: { name: string; color: string }[] = [
  { name: "Mauve", color: "#cba6f7" },
  { name: "Lavender", color: "#b4befe" },
  { name: "Blue", color: "#89b4fa" },
  { name: "Sapphire", color: "#74c7ec" },
  { name: "Teal", color: "#94e2d5" },
  { name: "Green", color: "#a6e3a1" },
  { name: "Yellow", color: "#f9e2af" },
  { name: "Peach", color: "#fab387" },
  { name: "Red", color: "#f38ba8" },
  { name: "Pink", color: "#f5c2e7" },
];

/** Whether text is a colour as the config stores one: `#rrggbb`. */
export function isHexColor(text: string): boolean {
  return /^#[0-9a-f]{6}$/i.test(text);
}

/**
 * How bright a colour looks, 0 to 1: the WCAG relative luminance, which
 * weighs green far above blue because the eye does.
 */
function luminance(hex: string): number {
  const channel = (at: number) => {
    const value = parseInt(hex.slice(at, at + 2), 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

/** The variables an accent colour sets. Empty for something that is not one. */
export function accentVars(accent: string): Record<string, string> {
  if (!isHexColor(accent)) return {};
  return {
    "--accent": accent,
    // Washes for filled backgrounds: a selected row, an open menu.
    "--accent-soft": `${accent}26`,
    "--hover-strong": `${accent}33`,
    // Text on a button filled with the accent: whichever of dark and light
    // reads on it. A pale accent wants dark ink, a deep one light.
    "--accent-ink": luminance(accent) > 0.36 ? "#1c1c1c" : "#f4f4f4",
  };
}

/** Put an accent into effect across the window. */
export function applyAccent(accent: string) {
  for (const [name, value] of Object.entries(accentVars(accent))) {
    document.documentElement.style.setProperty(name, value);
  }
}
