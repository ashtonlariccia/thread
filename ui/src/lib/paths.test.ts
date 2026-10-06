import { describe, expect, it } from "vitest";

import { baseName, dirName, samePath, segmentsBelow } from "./paths";

describe("dirName", () => {
  it("drops the last component, with either slash", () => {
    expect(dirName("C:\\src\\thread\\main.rs")).toBe("C:\\src\\thread");
    expect(dirName("C:/src/thread/main.rs")).toBe("C:/src/thread");
  });

  it("keeps the slash of a drive root", () => {
    expect(dirName("C:\\main.rs")).toBe("C:\\");
  });

  it("takes a remote path apart the same way", () => {
    expect(dirName("/home/ash/main.rs")).toBe("/home/ash");
    expect(dirName("/a/b")).toBe("/a");
    expect(dirName("/main.rs")).toBe("/");
  });

  it("is empty for a bare name", () => {
    expect(dirName("main.rs")).toBe("");
  });
});

describe("segmentsBelow", () => {
  it("lists the names from the root down to the file", () => {
    expect(segmentsBelow("C:\\src", "C:\\src\\lib\\state\\a.ts")).toEqual(["lib", "state", "a.ts"]);
  });

  it("ignores case, slash direction and a trailing slash on the root", () => {
    expect(segmentsBelow("c:/SRC/", "C:\\src\\Lib\\a.ts")).toEqual(["Lib", "a.ts"]);
  });

  it("is null for a path outside the root", () => {
    expect(segmentsBelow("C:\\src", "C:\\other\\a.ts")).toBeNull();
    // A sibling whose name merely starts the same way is not inside it.
    expect(segmentsBelow("C:\\src", "C:\\src-old\\a.ts")).toBeNull();
  });

  it("is exact about a remote's paths, where case tells files apart", () => {
    expect(segmentsBelow("/srv/app", "/srv/app/src/Main.py")).toEqual(["src", "Main.py"]);
    expect(segmentsBelow("/srv/App", "/srv/app/src/main.py")).toBeNull();
    expect(samePath("/srv/Makefile", "/srv/makefile")).toBe(false);
  });

  it("is null for the root itself", () => {
    expect(segmentsBelow("C:\\src", "C:\\src")).toBeNull();
  });
});

describe("baseName", () => {
  it("takes the last component with either slash", () => {
    expect(baseName("C:\\src\\thread\\main.rs")).toBe("main.rs");
    expect(baseName("C:/src/thread/main.rs")).toBe("main.rs");
    expect(baseName("C:\\src/mixed\\main.rs")).toBe("main.rs");
  });

  it("returns a bare name unchanged", () => {
    expect(baseName("main.rs")).toBe("main.rs");
  });
});

describe("samePath", () => {
  it("ignores case and slash direction", () => {
    expect(samePath("C:\\Src\\Main.rs", "c:/src/main.rs")).toBe(true);
  });

  it("tells different files apart", () => {
    expect(samePath("C:\\src\\main.rs", "C:\\src\\lib.rs")).toBe(false);
  });
});
