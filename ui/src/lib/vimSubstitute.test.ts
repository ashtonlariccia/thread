import { Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import { parseSubstitute, previewSubstitute, vimRegexToJs } from "./vimSubstitute";

describe("vimRegexToJs", () => {
  const matches = (pattern: string, text: string) =>
    new RegExp(vimRegexToJs(pattern)).exec(text)?.[0] ?? null;

  it("treats brackets and repeats as literal until escaped, as Vim does", () => {
    expect(matches("foo(", "x = foo(1)")).toBe("foo(");
    expect(matches("a+b", "a+b aab")).toBe("a+b");
    expect(matches("a\\+b", "a+b aab")).toBe("aab");
  });

  it("reads escaped brackets and bars as groups and alternatives", () => {
    expect(matches("\\(cat\\|dog\\)s", "two dogs")).toBe("dogs");
  });

  it("keeps the characters that are special in both", () => {
    expect(matches("f.o*", "fxooo")).toBe("fxooo");
    expect(matches("^int", "int x")).toBe("int");
    expect(matches("[0-9]", "abc7")).toBe("7");
  });

  it("understands word boundaries", () => {
    expect(matches("\\<f\\>", "foo f off")).toBe("f");
    expect(new RegExp(vimRegexToJs("\\<f\\>")).exec("foo f off")?.index).toBe(4);
  });

  it("switches to very magic with \\v", () => {
    expect(matches("\\v(cat|dog)s+", "dogss")).toBe("dogss");
  });

  it("limits the match with \\zs and \\ze", () => {
    expect(matches("foo\\zsbar", "foobar")).toBe("bar");
    expect(matches("foo\\zebar", "foobar")).toBe("foo");
  });
});

describe("parseSubstitute", () => {
  it("reads a whole-file substitute as it is typed", () => {
    expect(parseSubstitute("%s/f")).toEqual({
      range: { kind: "all" },
      pattern: "f",
      replacement: null,
      global: false,
      ignoreCase: false,
    });
    expect(parseSubstitute("%s/f/d")?.replacement).toBe("d");
    expect(parseSubstitute("%s/f/")?.replacement).toBe("");
    expect(parseSubstitute("%s/f/d/g")?.global).toBe(true);
    expect(parseSubstitute("%s/f/d/gi")?.ignoreCase).toBe(true);
  });

  it("is not a substitute until there is a pattern", () => {
    expect(parseSubstitute("%s")).toBeNull();
    expect(parseSubstitute("%s/")).toBeNull();
  });

  it("leaves other commands alone", () => {
    expect(parseSubstitute("w")).toBeNull();
    expect(parseSubstitute("set nu")).toBeNull();
    expect(parseSubstitute("sort")).toBeNull();
  });

  it("understands the ranges", () => {
    expect(parseSubstitute("s/a/b")?.range).toEqual({ kind: "current" });
    expect(parseSubstitute("'<,'>s/a/b")?.range).toEqual({ kind: "selection" });
    expect(parseSubstitute("3,7s/a/b")?.range).toEqual({ kind: "lines", from: 3, to: 7 });
    expect(parseSubstitute("5s/a/b")?.range).toEqual({ kind: "lines", from: 5, to: 5 });
    expect(parseSubstitute(".,$s/a/b", 4)?.range).toEqual({ kind: "lines", from: 4, to: Infinity });
  });

  it("accepts the long name and other delimiters", () => {
    expect(parseSubstitute("%substitute/a/b")?.pattern).toBe("a");
    expect(parseSubstitute("%s#a/b#c")).toMatchObject({ pattern: "a/b", replacement: "c" });
  });

  it("keeps an escaped delimiter inside the pattern", () => {
    expect(parseSubstitute("%s/a\\/b/c")).toMatchObject({ pattern: "a\\/b", replacement: "c" });
  });
});

describe("previewSubstitute", () => {
  const doc = Text.of(["int foo(float f) {", "  return f + off;", "}"]);
  const edits = (command: string, from = 1, to = doc.lines) =>
    previewSubstitute(doc, parseSubstitute(command)!, from, to).map(
      ({ from, to, insert }) => [doc.sliceString(from, to), insert] as const,
    );

  it("takes the first match on each line, as :s does without g", () => {
    expect(edits("%s/f/d")).toEqual([
      ["f", "d"],
      ["f", "d"],
    ]);
  });

  it("takes every match with g", () => {
    expect(edits("%s/f/d/g")).toHaveLength(6);
  });

  it("only highlights until there is a replacement", () => {
    expect(edits("%s/foo")).toEqual([["foo", null]]);
  });

  it("stays inside the lines it is given", () => {
    expect(edits("%s/f/d/g", 2, 2)).toHaveLength(3);
  });

  it("expands & and groups in the replacement", () => {
    expect(edits("%s/fo\\+/<&>")).toEqual([["foo", "<foo>"]]);
    expect(edits("%s/\\(f\\)\\(oo\\)/\\2\\1")).toEqual([["foo", "oof"]]);
  });

  it("previews nothing for a pattern that does not compile yet", () => {
    // A group that has been opened but not closed.
    expect(edits("%s/\\(f/d")).toEqual([]);
  });

  it("reads a bare bracket as a bracket, the way Vim does", () => {
    expect(edits("%s/(f/[")).toEqual([["(f", "["]]);
  });

  it("puts a literal & or backslash in with a backslash", () => {
    expect(edits("%s/foo/a\\&b")).toEqual([["foo", "a&b"]]);
    expect(edits("%s/foo/a\\\\b")).toEqual([["foo", "a\\b"]]);
  });

  it("does not loop for ever on a pattern that can match nothing", () => {
    expect(edits("%s/x*/d/g")).toEqual([]);
  });

  it("ignores case only when asked to", () => {
    expect(edits("%s/RETURN/x")).toEqual([]);
    expect(edits("%s/RETURN/x/i")).toEqual([["return", "x"]]);
  });
});
