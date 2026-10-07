import { describe, expect, it } from "vitest";

import { findLinks, resolveLink } from "./links";

/** What was found, as the text of each link. */
const texts = (line: string) => findLinks(line).map((link) => line.slice(link.start, link.end));

describe("links in terminal output", () => {
  it("finds a compiler's path, with its line and column", () => {
    expect(findLinks("error: src/main.rs:12:5: expected `;`")).toEqual([
      { start: 7, end: 23, path: "src/main.rs", line: 12, col: 5 },
    ]);
  });

  it("reads the other ways a line number is written", () => {
    expect(findLinks("C:\\src\\app\\a.cs(40,7): warning")[0]).toMatchObject({
      path: "C:\\src\\app\\a.cs",
      line: 40,
      col: 7,
    });
    expect(findLinks('  File "./tools/run.py", line 3')[0]).toMatchObject({ path: "./tools/run.py" });
    expect(findLinks("Makefile.inc:9")[0]).toMatchObject({ path: "Makefile.inc", line: 9 });
  });

  it("takes a bare file name for a word, unless it has a line after it", () => {
    expect(texts("see README.md for more, e.g. the end.")).toEqual([]);
    expect(texts("version 1.2.3 of node.js")).toEqual([]);
    expect(texts("at main.c:10")).toEqual(["main.c:10"]);
  });

  it("finds web addresses, less what the sentence ends with", () => {
    expect(findLinks("Docs: https://example.com/a/b.html?x=1.")).toEqual([
      { start: 6, end: 38, url: "https://example.com/a/b.html?x=1" },
    ]);
    // And does not find a file inside one.
    expect(texts("(see http://localhost:5174/src/main.ts)")).toEqual(["http://localhost:5174/src/main.ts"]);
  });

  it("finds several on a line, in order", () => {
    expect(texts("a/b.c:1 and c/d.h:2")).toEqual(["a/b.c:1", "c/d.h:2"]);
  });
});

describe("where a path from a terminal leads", () => {
  it("is itself when it is absolute", () => {
    expect(resolveLink("/srv/app/a.py", "/home/me")).toBe("/srv/app/a.py");
    expect(resolveLink("C:\\src\\a.rs", "D:\\work")).toBe("C:\\src\\a.rs");
  });

  it("is from the terminal's folder otherwise, in that folder's slashes", () => {
    expect(resolveLink("src/main.rs", "C:\\work\\thread")).toBe("C:\\work\\thread\\src\\main.rs");
    expect(resolveLink("./src/main.rs", "/home/me/thread/")).toBe("/home/me/thread/src/main.rs");
  });

  it("is left as it is with no folder to start from", () => {
    expect(resolveLink("src/main.rs", null)).toBe("src/main.rs");
  });
});
