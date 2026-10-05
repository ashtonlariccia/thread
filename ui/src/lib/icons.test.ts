import { describe, expect, it } from "vitest";

import files from "./icons/files.json";
import folders from "./icons/folders.json";
import { fileIconId, folderIconId, type FileIconMap, type FolderIconMap } from "./icons";

const map = files as unknown as FileIconMap;

const folderMap = folders as unknown as FolderIconMap;

describe("folderIconId", () => {
  it("gives well-known folders their own icon, open and closed", () => {
    expect(folderIconId(folderMap, "src", false)).toBe("folder-src");
    expect(folderIconId(folderMap, "SRC", true)).toBe("folder-src-open");
  });

  it("falls back to the plain folder", () => {
    expect(folderIconId(folderMap, "zzz-nothing-special", false)).toBe(folderMap.folder);
    expect(folderIconId(folderMap, "zzz-nothing-special", true)).toBe(folderMap.folderOpen);
  });

  it("marks the root as the root, whatever it is called", () => {
    expect(folderIconId(folderMap, "src", false, true)).toBe(folderMap.root);
    expect(folderIconId(folderMap, "src", true, true)).toBe(folderMap.rootOpen);
  });
});

describe("fileIconId", () => {
  it("matches common extensions from the imported pack", () => {
    expect(fileIconId(map, "main.rs")).toBe("rust");
    expect(fileIconId(map, "app.py")).toBe("python");
    expect(fileIconId(map, "index.ts")).toBe("typescript");
  });

  it("is case-insensitive", () => {
    expect(fileIconId(map, "MAIN.RS")).toBe("rust");
  });

  it("prefers the longest extension", () => {
    expect(fileIconId(map, "types.d.ts")).toBe("typescript-def");
  });

  it("prefers a whole-name match over the extension", () => {
    const tiny: FileIconMap = { file: "file", extensions: { json: "json" }, names: { "package.json": "nodejs" } };
    expect(fileIconId(tiny, "package.json")).toBe("nodejs");
    expect(fileIconId(tiny, "other.json")).toBe("json");
  });

  it("matches a dotfile by what follows the dot", () => {
    const tiny: FileIconMap = { file: "file", extensions: { gitignore: "git" }, names: {} };
    expect(fileIconId(tiny, ".gitignore")).toBe("git");
  });

  it("falls back to the plain file icon", () => {
    expect(fileIconId(map, "no-extension-at-all")).toBe(map.file);
    expect(fileIconId(map, "thing.zzzunknownzzz")).toBe(map.file);
  });
});
