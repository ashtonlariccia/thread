import { describe, expect, it } from "vitest";

import { EMPTY_SYNTAX, patternError, tokens, type SyntaxDef } from "./customSyntax";
import { languageOf, setCustomLanguages } from "./languages";

const def: SyntaxDef = {
  ...EMPTY_SYNTAX,
  extensions: ["my"],
  line_comment: "#",
  block_comment: ["/*", "*/"],
  strings: ['"', '"""'],
  keywords: ["if", "let"],
  types: ["int"],
  constants: ["true"],
  functions: ["print"],
  patterns: [{ match: "@[a-z]+", as: "builtin" }],
};

describe("a language described in the config", () => {
  it("colours words by the list they are in", () => {
    expect(tokens(def, ["let x: int = true"])[0]).toEqual([
      ["let", "keyword"],
      ["x", "variableName"],
      [":", "operator"],
      ["int", "typeName"],
      ["=", "operator"],
      ["true", "constant"],
    ]);
  });

  it("knows a call from a name, and a listed function either way", () => {
    expect(tokens(def, ["foo(print, bar)"])[0]).toEqual([
      ["foo", "call"],
      ["(", "bracket"],
      ["print", "call"],
      [",", "punctuation"],
      ["bar", "variableName"],
      [")", "bracket"],
    ]);
  });

  it("reads comments, to the end of the line or across several", () => {
    const [first, second, third] = tokens(def, ["x # if let", "a /* if", "let */ b"]);
    expect(first).toEqual([
      ["x", "variableName"],
      ["# if let", "comment"],
    ]);
    expect(second.at(-1)).toEqual(["/* if", "comment"]);
    expect(third).toEqual([
      ["let */", "comment"],
      ["b", "variableName"],
    ]);
  });

  it("keeps a quote behind a backslash inside its string", () => {
    expect(tokens(def, ['"a \\" b" c'])[0]).toEqual([
      ['"a \\" b"', "string"],
      ["c", "variableName"],
    ]);
  });

  it("lets only a long delimiter run on to the next line", () => {
    const [, short] = tokens(def, ['"open', "let"]);
    expect(short).toEqual([["let", "keyword"]]);

    const [, long] = tokens(def, ['"""open', 'still""" let']);
    expect(long).toEqual([
      ['still"""', "string"],
      ["let", "keyword"],
    ]);
  });

  it("tries patterns before words and numbers", () => {
    expect(tokens(def, ["@inline 0xFF 1.5e3"])[0]).toEqual([
      ["@inline", "builtin"],
      ["0xFF", "number"],
      ["1.5e3", "number"],
    ]);
  });

  it("colours only the group of a pattern that has one", () => {
    const grouped = {
      ...def,
      keywords: ["ds"],
      patterns: [{ match: "ds\\s+(\\w+)", as: "type" }, { match: "(@)[a-z]+", as: "operator" }],
    };
    expect(tokens(grouped, ["ds point {}", "ds", "dsx y @at"])).toEqual([
      [
        ["ds", "keyword"],
        ["point", "typeName"],
        ["{", "bracket"],
        ["}", "bracket"],
      ],
      [["ds", "keyword"]],
      [
        ["dsx", "variableName"],
        ["y", "variableName"],
        ["@", "operator"],
        ["at", "variableName"],
      ],
    ]);
  });

  it("stops what is before a group where the group starts", () => {
    const tight = { ...def, patterns: [{ match: "a(b)", as: "type" }] };
    expect(tokens(tight, ["ab c"])[0]).toEqual([
      ["a", "variableName"],
      ["b", "typeName"],
      ["c", "variableName"],
    ]);
  });

  it("reads what a string interpolates as code, and the rest as string", () => {
    const embedding = { ...def, interpolation: ["{", "}"] };
    expect(tokens(embedding, ['"a {if x} b" let', '"\\{x} {', "let"])).toEqual([
      [
        ['"a ', "string"],
        ["{", "bracket"],
        ["if", "keyword"],
        ["x", "variableName"],
        ["}", "bracket"],
        [' b"', "string"],
        ["let", "keyword"],
      ],
      [
        ['"\\{x} ', "string"],
        ["{", "bracket"],
      ],
      [["let", "keyword"]],
    ]);
  });

  it("closes a string's code where its own opener is closed", () => {
    const embedding = { ...def, interpolation: ["{", "}"] };
    expect(tokens(embedding, ['"{ {a} "b" }c"'])[0]).toEqual([
      ['"', "string"],
      ["{", "bracket"],
      ["{", "bracket"],
      ["a", "variableName"],
      ["}", "bracket"],
      ['"b"', "string"],
      ["}", "bracket"],
      ['c"', "string"],
    ]);
  });

  it("ignores a pattern that is not a pattern, or matches nothing", () => {
    const broken = { ...def, patterns: [{ match: "(", as: "keyword" }, { match: "x*", as: "keyword" }] };
    expect(patternError("(")).not.toBeNull();
    expect(tokens(broken, ["let y"])[0]).toEqual([
      ["let", "keyword"],
      ["y", "variableName"],
    ]);
  });
});

describe("a file extension claimed by a language of the user's own", () => {
  it("is that language, ahead of what Tide would have said", () => {
    setCustomLanguages({ Mylang: ["my", "rs"] });
    expect(languageOf("a.my")).toBe("Mylang");
    expect(languageOf("main.RS")).toBe("Mylang");

    setCustomLanguages({});
    expect(languageOf("a.my")).toBe("Plain Text");
    expect(languageOf("main.rs")).toBe("Rust");
  });
});
