import { describe, expect, it, vi } from "vitest";

// The module draws with xterm, which wants a browser; only the reading of a
// request is under test here.
vi.mock("@xterm/xterm", () => ({ Terminal: class {} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

import { openRequest } from "./terminal";

describe("the thread command's request", () => {
  it("says what to open and where", () => {
    expect(openRequest("file;/home/me/a.c")).toEqual({ kind: "file", path: "/home/me/a.c" });
    expect(openRequest("dir;/srv/my app")).toEqual({ kind: "dir", path: "/srv/my app" });
    expect(openRequest("close;/srv/my app")).toEqual({ kind: "close", path: "/srv/my app" });
  });

  it("keeps a semicolon that is part of the path", () => {
    expect(openRequest("file;/tmp/a;b.txt")?.path).toBe("/tmp/a;b.txt");
  });

  it("is nothing if it is not one", () => {
    expect(openRequest("")).toBeNull();
    expect(openRequest("file;")).toBeNull();
    expect(openRequest("run;/bin/sh")).toBeNull();
    expect(openRequest("/home/me/a.c")).toBeNull();
  });
});
