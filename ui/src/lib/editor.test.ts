// @vitest-environment happy-dom
import { EditorView } from "@codemirror/view";
import { describe, expect, it } from "vitest";

import { EditorHost } from "./editor";
import type { Indent } from "./indent";

const INDENT: Indent = { width: 4, spaces: true };
const FILE = 1;

/** A host with two panes, both on one file that says `text`. */
function split(text = "one\ntwo\n") {
  const dirty: boolean[] = [];
  const host = new EditorHost({
    ondirty: (_key, is) => dirty.push(is),
    onchange: () => {},
    oncursor: () => {},
    onview: () => {},
    onfocus: () => {},
  });
  const views = [1, 2].map((pane) => {
    const parent = document.createElement("div");
    document.body.append(parent);
    host.mount(pane, parent);
    return () => EditorView.findFromDOM(parent)!;
  });
  host.create(FILE, text, INDENT);
  host.show(1, FILE);
  host.show(2, FILE);
  const [left, right] = views;
  const type = (view: () => EditorView, at: number, insert: string) =>
    view().dispatch({ changes: { from: at, insert }, userEvent: "input.type" });
  return { host, left, right, type, dirty };
}

/** Edits a moment apart are one step to undo; these are made apart. */
const apart = () => new Promise((resolve) => setTimeout(resolve, 600));

describe("a file open in two panes", () => {
  it("says the same thing in both, whichever is typed into", () => {
    const { host, left, right, type } = split();
    type(left, 0, "A");
    type(right, 0, "B");
    expect(left().state.doc.toString()).toBe("BAone\ntwo\n");
    expect(right().state.doc.toString()).toBe("BAone\ntwo\n");
    expect(host.text(FILE)).toBe("BAone\ntwo\n");
  });

  it("undoes the last thing done to the file, in whichever pane that was", async () => {
    const { host, left, right, type } = split();
    type(left, 0, "A");
    await apart();
    type(right, 0, "B");

    // Asked for in the left pane, the edit that goes is the right pane's.
    expect(host.travelIn(left(), "undo")).toBe(true);
    expect(host.text(FILE)).toBe("Aone\ntwo\n");
    expect(right().state.doc.toString()).toBe("Aone\ntwo\n");

    expect(host.travelIn(right(), "undo")).toBe(true);
    expect(host.text(FILE)).toBe("one\ntwo\n");
    // Nothing left; and the two have not drifted apart finding that out.
    expect(host.travelIn(left(), "undo")).toBe(false);
    expect(left().state.doc.eq(right().state.doc)).toBe(true);
  });

  it("redoes from either pane what was undone from the other", async () => {
    const { host, left, right, type } = split();
    type(left, 3, "!");
    host.travelIn(right(), "undo");
    expect(host.text(FILE)).toBe("one\ntwo\n");
    expect(host.travelIn(left(), "redo")).toBe(true);
    expect(host.text(FILE)).toBe("one!\ntwo\n");
    expect(right().state.doc.toString()).toBe("one!\ntwo\n");
  });

  it("leaves the cursor at what was undone in the pane that asked", () => {
    const { host, left, right, type } = split();
    type(right, 4, "X");
    host.travelIn(left(), "undo");
    expect(left().state.selection.main.head).toBe(4);
  });

  it("undoes a run of typing in one step", () => {
    const { host, left, type } = split("");
    type(left, 0, "a");
    type(left, 1, "b");
    type(left, 2, "c");
    host.travelIn(left(), "undo");
    expect(host.text(FILE)).toBe("");
  });

  it("is unsaved while it differs from what was saved, and not once undone", () => {
    const { host, left, type, dirty } = split();
    type(left, 0, "A");
    expect(dirty.at(-1)).toBe(true);
    host.travelIn(left(), "undo");
    expect(dirty.at(-1)).toBe(false);
  });
});

describe("a file in a pane that lets go of it", () => {
  it("can still be undone from the pane that has it", () => {
    const { host, left, right, type } = split();
    type(right, 0, "B");
    host.release(2, FILE);
    host.show(2, null);
    expect(host.travelIn(left(), "undo")).toBe(true);
    expect(host.text(FILE)).toBe("one\ntwo\n");
  });

  it("keeps up with edits while no pane is showing it", () => {
    const { host, left, type } = split();
    host.show(2, null);
    type(left, 0, "A");
    host.show(2, FILE);
    expect(host.text(FILE)).toBe("Aone\ntwo\n");
  });

  it("is reloaded from disk as an edit that can be undone", () => {
    const { host, left } = split();
    host.replace(FILE, "changed\n");
    expect(left().state.doc.toString()).toBe("changed\n");
    host.travelIn(left(), "undo");
    expect(host.text(FILE)).toBe("one\ntwo\n");
  });
});
