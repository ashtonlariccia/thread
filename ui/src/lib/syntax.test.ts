import { describe, expect, it } from "vitest";

import { languageOf, PLAIN_TEXT } from "./languages";
import { hasSyntax, loadSyntax } from "./syntax";
import { DEFAULT_THEME, syntaxTheme, THEME_NAMES } from "./themes";

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
