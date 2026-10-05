import { describe, expect, it } from "vitest";

import { detectIndent, indentLabel, resolveIndent, type IndentPreference } from "./indent";

describe("detectIndent", () => {
  it("has nothing to say about a file with no indentation", () => {
    expect(detectIndent("")).toBeNull();
    expect(detectIndent("a\nb\nc\n")).toBeNull();
  });

  it("recognises tabs", () => {
    expect(detectIndent("fn main() {\n\tlet x = 1;\n\tif x {\n\t\ty();\n\t}\n}\n")).toEqual({
      spaces: false,
      width: null,
    });
  });

  it("measures the step, not the depth", () => {
    // Nested to eight columns, two at a time.
    const text = "a:\n  b:\n    c:\n      d:\n        e\n";
    expect(detectIndent(text)).toEqual({ spaces: true, width: 2 });
  });

  it("recognises four spaces", () => {
    const text = "def f():\n    if x:\n        return 1\n    return 2\n";
    expect(detectIndent(text)).toEqual({ spaces: true, width: 4 });
  });

  it("is not thrown by the one-space lines of a block comment", () => {
    const text = "/**\n * A comment.\n * More of it.\n */\nfn f() {\n    g();\n    h();\n}\n";
    expect(detectIndent(text)).toEqual({ spaces: true, width: 4 });
  });

  it("goes with the majority in a file that mixes the two", () => {
    const text = "a {\n\tb;\n\tc;\n\td;\n    e;\n}\n";
    expect(detectIndent(text)?.spaces).toBe(false);
  });

  it("handles CRLF text and a last line with no newline", () => {
    expect(detectIndent("a {\r\n  b;\r\n  c;\r\n}")).toEqual({ spaces: true, width: 2 });
  });
});

describe("resolveIndent", () => {
  const preference: IndentPreference = {
    tab_width: 4,
    insert_spaces: true,
    detect_indentation: true,
  };

  it("uses the config when the file says nothing", () => {
    expect(resolveIndent(preference, null)).toEqual({ spaces: true, width: 4 });
  });

  it("follows the file when it has habits of its own", () => {
    expect(resolveIndent(preference, { spaces: true, width: 2 })).toEqual({ spaces: true, width: 2 });
  });

  it("keeps the configured width for a file indented with tabs", () => {
    expect(resolveIndent(preference, { spaces: false, width: null })).toEqual({
      spaces: false,
      width: 4,
    });
  });

  it("ignores the file when detection is off", () => {
    const fixed = { ...preference, detect_indentation: false };
    expect(resolveIndent(fixed, { spaces: false, width: null })).toEqual({ spaces: true, width: 4 });
  });
});

describe("indentLabel", () => {
  it("reads as the bottom bar shows it", () => {
    expect(indentLabel({ spaces: true, width: 2 })).toBe("Spaces: 2");
    expect(indentLabel({ spaces: false, width: 4 })).toBe("Tabs: 4");
  });
});
