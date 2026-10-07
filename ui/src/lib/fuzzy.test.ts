import { describe, expect, it } from "vitest";

import { fuzzy, rank } from "./fuzzy";

const COMMANDS = [
  "File: New File",
  "File: Save",
  "Terminal: New Terminal",
  "View: Split Right",
  "Settings: Turn Word Wrap On",
];
const best = (query: string) => rank(query, COMMANDS, (name) => name)[0]?.item;

describe("finding a command by what was typed", () => {
  it("takes letters in order, with anything between them", () => {
    expect(fuzzy("nwtrm", "New Terminal")).not.toBeNull();
    expect(fuzzy("mret", "New Terminal")).toBeNull();
  });

  it("ignores case, and spaces in what was typed", () => {
    expect(fuzzy("NEW term", "File: new terminal")?.at.length).toBe(7);
  });

  it("says where each letter was found", () => {
    expect(fuzzy("nt", "New Terminal")?.at).toEqual([0, 4]);
  });

  it("puts initials and whole words ahead of scattered letters", () => {
    expect(best("nt")).toBe("Terminal: New Terminal");
    expect(best("split")).toBe("View: Split Right");
    expect(best("save")).toBe("File: Save");
    expect(best("wrap")).toBe("Settings: Turn Word Wrap On");
  });

  it("gives everything back, as it came, for nothing typed", () => {
    expect(rank("", COMMANDS, (name) => name).map((found) => found.item)).toEqual(COMMANDS);
  });

  it("gives nothing back when nothing matches", () => {
    expect(rank("zzz", COMMANDS, (name) => name)).toEqual([]);
  });
});
