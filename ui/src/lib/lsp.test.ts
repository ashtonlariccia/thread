import { EditorState, Text } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import { diagnostics, drawn, lensDetail, setDiagnostics } from "./diagnostics";
import { toCompletions, toDiagnostic, toOffset, toPosition, toUri, uriKey } from "./lsp";

const doc = Text.of(["fn main() {", "    let x = 1;", "}"]);

describe("paths as the protocol writes them", () => {
  it("keeps the drive's colon and escapes what a URI cannot hold", () => {
    expect(toUri("C:\\src\\my app\\a#b.rs")).toBe("file:///C:/src/my%20app/a%23b.rs");
    expect(toUri("/srv/app/main.py")).toBe("file:///srv/app/main.py");
  });

  it("knows a server's spelling of a file from ours", () => {
    expect(uriKey("file:///c%3A/src/My%20App/a.rs")).toBe(uriKey(toUri("C:\\src\\My App\\a.rs")));
  });
});

describe("places in a file", () => {
  it("turns an offset into a line and a character, and back", () => {
    const offset = doc.line(2).from + 8;
    expect(toPosition(doc, offset)).toEqual({ line: 1, character: 8 });
    expect(toOffset(doc, { line: 1, character: 8 })).toBe(offset);
  });

  it("keeps a place the text no longer has inside the text", () => {
    expect(toOffset(doc, { line: 1, character: 999 })).toBe(doc.line(2).to);
    expect(toOffset(doc, { line: 40, character: 0 })).toBe(doc.length);
  });
});

describe("completions", () => {
  it("keeps the server's ranking", () => {
    const options = toCompletions([
      { label: "zeta", sortText: "0" },
      { label: "alpha", sortText: "1" },
    ]);
    expect(options.map((o) => o.label)).toEqual(["zeta", "alpha"]);
    expect(options[0].boost).toBeGreaterThan(options[1].boost!);
  });

  it("inserts what the server said to, which need not be the label", () => {
    const [option] = toCompletions([{ label: "println!(…)", insertText: "println!", kind: 3 }]);
    expect(option.apply).toBe("println!");
    expect(option.type).toBe("function");
  });
});

describe("what a server finds wrong", () => {
  const wire = (line: number, from: number, to: number, severity: number, message: string) => ({
    range: { start: { line, character: from }, end: { line, character: to } },
    severity,
    message,
  });

  it("is underlined where it is and written out at the end of its line", () => {
    const state = EditorState.create({ doc, extensions: diagnostics });
    const found = [wire(1, 8, 9, 2, "unused variable: `x`")].map((w) => toDiagnostic(doc, w));
    const line = doc.line(2);

    expect(drawn(state.update({ effects: setDiagnostics.of(found) }).state)).toEqual([
      [line.from, line.from, "cm-lens-line cm-lens-line-warning", "unused variable: `x`"],
      [line.from + 8, line.from + 9, "cm-lens-range cm-lens-range-warning"],
    ]);
  });

  it("underlines a hint and writes nothing after its line", () => {
    const state = EditorState.create({ doc, extensions: diagnostics });
    const found = [toDiagnostic(doc, wire(0, 3, 7, 4, "could be shorter"))];
    expect(drawn(state.update({ effects: setDiagnostics.of(found) }).state)).toEqual([
      [3, 7, "cm-lens-range cm-lens-range-hint"],
    ]);
  });

  it("writes out the most serious on a line, on one line", () => {
    const state = EditorState.create({ doc, extensions: diagnostics });
    const found = [wire(1, 8, 9, 2, "unused"), wire(1, 12, 13, 1, "mismatched types\nexpected `()`")];
    const next = state.update({ effects: setDiagnostics.of(found.map((w) => toDiagnostic(doc, w))) });

    const [line] = drawn(next.state);
    expect(line.slice(2)).toEqual([
      "cm-lens-line cm-lens-line-error",
      "mismatched types ⏎ expected `()`",
    ]);
    // And all of them, in full, for when the line is opened.
    expect(lensDetail(found.map((w) => toDiagnostic(doc, w)))).toBe(
      "mismatched types\nexpected `()`\nunused",
    );
  });

  it("goes when the server says there is nothing wrong any more", () => {
    const state = EditorState.create({ doc, extensions: diagnostics });
    const found = [toDiagnostic(doc, wire(0, 3, 7, 1, "bad"))];
    const marked = state.update({ effects: setDiagnostics.of(found) }).state;
    expect(drawn(marked.update({ effects: setDiagnostics.of([]) }).state)).toEqual([]);
  });
});
