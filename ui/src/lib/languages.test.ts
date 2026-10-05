import { describe, expect, it } from "vitest";

import { languageOf, PLAIN_TEXT } from "./languages";

describe("languageOf", () => {
  it("names a language by extension, whatever the case", () => {
    expect(languageOf("main.rs")).toBe("Rust");
    expect(languageOf("App.SVELTE")).toBe("Svelte");
    expect(languageOf("package.json")).toBe("JSON");
  });

  it("uses the final extension", () => {
    expect(languageOf("types.d.ts")).toBe("TypeScript");
    expect(languageOf("archive.tar.md")).toBe("Markdown");
  });

  it("recognises files known by their whole name", () => {
    expect(languageOf("Dockerfile")).toBe("Dockerfile");
    expect(languageOf("Cargo.lock")).toBe("TOML");
    expect(languageOf(".gitignore")).toBe("Ignore");
  });

  it("calls anything else plain text", () => {
    expect(languageOf("notes")).toBe(PLAIN_TEXT);
    expect(languageOf("thing.zzz")).toBe(PLAIN_TEXT);
  });
});
