import { describe, expect, it } from "vitest";

import { languageOf, PLAIN_TEXT } from "./languages";
import { hasSyntax, loadSyntax } from "./syntax";
import { carryAccent, DEFAULT_THEME, paletteOf, syntaxTheme, THEME_NAMES, THEMES } from "./themes";

describe("grammars", () => {
  it("exist for the languages people actually open", () => {
    for (const file of ["main.rs", "app.ts", "App.svelte", "x.py", "Cargo.toml", "a.json", "b.md", "c.cpp", "run.sh"]) {
      expect(hasSyntax(languageOf(file)), file).toBe(true);
    }
  });

  it("are not claimed for plain text", () => {
    expect(hasSyntax(PLAIN_TEXT)).toBe(false);
  });

  it("load to something usable, by either route", async () => {
    // One proper parser, one legacy tokeniser.
    expect(await loadSyntax("Rust")).not.toBeNull();
    expect(await loadSyntax("TOML")).not.toBeNull();
  });

  it("resolve to null, not an error, for a language with none", async () => {
    await expect(loadSyntax(PLAIN_TEXT)).resolves.toBeNull();
    await expect(loadSyntax("No Such Language")).resolves.toBeNull();
  });
});

describe("syntax themes", () => {
  it("include the default", () => {
    expect(THEME_NAMES).toContain(DEFAULT_THEME);
  });

  it("are found whatever the case or spacing", () => {
    expect(syntaxTheme(" Catppuccin ")).toBe(syntaxTheme("catppuccin"));
  });

  it("fall back to the default for a name that is not a theme", () => {
    expect(syntaxTheme("no-such-theme")).toBe(syntaxTheme(DEFAULT_THEME));
  });
});

describe("colour themes", () => {
  it("each have a dark and a light, made up differently", () => {
    for (const theme of THEMES) {
      expect(syntaxTheme(theme.name, "light"), theme.name).not.toBe(syntaxTheme(theme.name, "dark"));
      expect(theme.light.accents.length, theme.name).toBeGreaterThan(0);
      expect(theme.dark.accents.length, theme.name).toBeGreaterThan(0);
    }
  });

  it("are dark unless asked to be light", () => {
    expect(paletteOf("gruvbox", "nonsense")).toBe(paletteOf("gruvbox", "dark"));
  });

  it("carry a preset accent across, and leave a chosen one alone", () => {
    const mocha = paletteOf("catppuccin", "dark");
    const latte = paletteOf("catppuccin", "light");
    expect(carryAccent("#cba6f7", mocha, latte)).toBe("#8839ef");
    expect(carryAccent("#CBA6F7", mocha, paletteOf("gruvbox", "dark"))).toBe("#fe8019");
    expect(carryAccent("#123456", mocha, latte)).toBe("#123456");
  });
});
