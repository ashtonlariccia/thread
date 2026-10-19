import { describe, expect, it } from "vitest";

import { headersFirst } from "./treeOrder";

const files = (...names: string[]) => names.map((name) => ({ name, dir: false }));
const names = (entries: { name: string }[]) => entries.map((e) => e.name);

describe("headersFirst", () => {
  it("puts a header just above its source", () => {
    expect(names(headersFirst(files("main.c", "main.h", "util.c", "util.h")))).toEqual([
      "main.h",
      "main.c",
      "util.h",
      "util.c",
    ]);
  });

  it("brings the header up past what lies between", () => {
    expect(names(headersFirst(files("io.c", "io.cpp", "io.d", "io.h")))).toEqual([
      "io.h",
      "io.c",
      "io.cpp",
      "io.d",
    ]);
  });

  it("leaves a header with no source where it is", () => {
    expect(names(headersFirst(files("a.c", "config.h", "z.txt")))).toEqual(["a.c", "config.h", "z.txt"]);
  });

  it("pairs them without regard to case", () => {
    expect(names(headersFirst(files("Main.C", "main.h")))).toEqual(["main.h", "Main.C"]);
  });

  it("leaves folders alone", () => {
    const entries = [{ name: "x.h", dir: true }, ...files("x.c")];
    expect(headersFirst(entries)).toEqual(entries);
  });
});
