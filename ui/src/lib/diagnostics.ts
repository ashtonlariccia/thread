/**
 * What a language server has to say about a file, drawn where it applies.
 *
 * As the VS Code extension Error Lens draws it, at that extension's
 * defaults: a line with a problem is tinted, and the problem is written out
 * after the end of it, four characters on, in the colour of its severity.
 * One message to a line, the most serious there, on one line however long it
 * is: what does not fit is cut off at the edge of the editor. Hints are
 * underlined and not written out. The colours are the extension's own.
 *
 * One thing is added. On the line the cursor is on, or the one being pointed
 * at, the message opens into a box under the line with everything said about
 * the line, in full, wrapped to the width of the editor: which is how a
 * message too long for a small window gets read.
 *
 * None of it is text in the editor. Both are drawn by the line's own
 * stylesheet, from attributes, so there is nothing there for the cursor to
 * be beside, for a selection to take in, or for a copy to pick up; and the
 * box is laid over the lines below rather than among them, so opening it
 * moves nothing.
 *
 * This only draws. What the complaints are is whoever sets them's business.
 */
import { StateEffect, StateField, type Extension, type Text } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView } from "@codemirror/view";

/** Most serious first, which is also the order they are compared in. */
export const SEVERITIES = ["error", "warning", "info", "hint"] as const;
export type Severity = (typeof SEVERITIES)[number];

export type Diagnostic = {
  /** Offsets into the text. */
  from: number;
  to: number;
  severity: Severity;
  message: string;
  /** Who says so: the server, or the tool behind it. */
  source?: string;
};

/** Replace everything said about a file. */
export const setDiagnostics = StateEffect.define<Diagnostic[]>();

/** As much of a message as is written after a line (`errorLens.messageMaxChars`). */
const LENS_CHARS = 500;
/** What stands for a line break in a message written on one line. */
const LINE_BREAK = "⏎";

const rank = (d: Diagnostic) => SEVERITIES.indexOf(d.severity);

/** A message as it is written after its line: all on one line. */
export function lensText(message: string): string {
  const flat = message.trim().replace(/\s*\n\s*/g, ` ${LINE_BREAK} `);
  return flat.length > LENS_CHARS ? `${flat.slice(0, LENS_CHARS)}…` : flat;
}

/** Everything said about a line, in full: the most serious first, one to a paragraph. */
export function lensDetail(all: Diagnostic[]): string {
  return [...all]
    .sort((a, b) => rank(a) - rank(b))
    .map((d) => (d.source ? `${d.message.trim()} (${d.source})` : d.message.trim()))
    .join("\n");
}

function build(doc: Text, diagnostics: Diagnostic[]): DecorationSet {
  const lines = new Map<number, Diagnostic[]>();
  const marks: { from: number; to: number; severity: Severity }[] = [];

  for (const diagnostic of diagnostics) {
    const from = Math.min(Math.max(0, diagnostic.from), doc.length);
    const line = doc.lineAt(from);
    let to = Math.min(Math.max(from, diagnostic.to), doc.length);
    // A complaint about a point is drawn under the character at it.
    if (to === from) to = Math.min(from + 1, line.to);
    if (to > from) marks.push({ from, to, severity: diagnostic.severity });

    // A hint is a suggestion, not a problem: underlined, and left at that.
    if (diagnostic.severity === "hint") continue;
    const known = lines.get(line.from);
    if (known) known.push(diagnostic);
    else lines.set(line.from, [diagnostic]);
  }

  const ranges = [
    ...marks.map(({ from, to, severity }) =>
      Decoration.mark({ class: `cm-lens-range cm-lens-range-${severity}` }).range(from, to),
    ),
    ...[...lines].map(([from, all]) => {
      // The first of the most serious, as they came.
      const worst = all.reduce((a, b) => (rank(b) < rank(a) ? b : a));
      return Decoration.line({
        class: `cm-lens-line cm-lens-line-${worst.severity}`,
        attributes: { "data-lens": lensText(worst.message), "data-lens-detail": lensDetail(all) },
      }).range(from);
    }),
  ];
  return ranges.length === 0 ? Decoration.none : Decoration.set(ranges, true);
}

const field = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(set, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setDiagnostics)) return build(tr.state.doc, effect.value);
    }
    // Until they are replaced, they stay with the text they were about.
    return tr.docChanged ? set.map(tr.changes) : set;
  },
  provide: (self) => EditorView.decorations.from(self),
});

/** Error Lens's own, for a dark theme: the message, and the wash behind its line. */
const COLOURS: Record<Severity, { text: string; line: string }> = {
  error: { text: "#ff6464", line: "#e454541b" },
  warning: { text: "#fa973a", line: "#ff942f1b" },
  info: { text: "#00b7e4", line: "#00b7e420" },
  hint: { text: "#2faf64", line: "#17a2a220" },
};

/** The line the message opens on: the one pointed at, and the cursor's. */
const OPEN = [".cm-lens-line:hover::after", "&.cm-focused .cm-activeLine.cm-lens-line::after"].join(", ");

const look = EditorView.baseTheme({
  // The message after the line. Out of the flow, though it sits where it
  // would have in it, so that a long one makes the line no wider: what does
  // not fit is cut off where the line ends, rather than giving the editor
  // somewhere to scroll sideways to.
  ".cm-lens-line": { position: "relative", overflowX: "clip" },
  ".cm-lens-line::after": {
    content: "attr(data-lens)",
    position: "absolute",
    marginLeft: "4ch",
    whiteSpace: "pre",
  },
  // Opened: everything said about the line, in a box hung under it. Taken
  // out of the flow, so it is as tall as it needs to be and nothing moves to
  // make room for it; and as wide as what it says, up to the width of the
  // editor it is in, which is what wraps it to a small window or a narrow
  // pane. The pointer goes through it, so the lines it lies over can still
  // be pointed at and clicked.
  ".cm-scroller": { containerType: "inline-size" },
  [OPEN]: {
    content: "attr(data-lens-detail)",
    position: "absolute",
    top: "100%",
    left: "2ch",
    zIndex: "5",
    width: "max-content",
    maxWidth: "calc(100cqw - 9ch)",
    margin: "0",
    padding: "1px 0.8em",
    borderRadius: "0.2em",
    border: "1px solid",
    backgroundColor: "var(--bg-menu)",
    boxShadow: "0 6px 18px #0008",
    whiteSpace: "pre-wrap",
    overflowWrap: "anywhere",
    pointerEvents: "none",
  },
  ".cm-lens-range": {
    textDecorationLine: "underline",
    textDecorationStyle: "wavy",
    textDecorationSkipInk: "none",
    textUnderlineOffset: "3px",
  },
  ...Object.fromEntries(
    SEVERITIES.flatMap((severity) => [
      [`.cm-lens-line-${severity}`, { backgroundColor: COLOURS[severity].line }],
      [`.cm-lens-line-${severity}::after`, { color: COLOURS[severity].text }],
      [`.cm-lens-range-${severity}`, { textDecorationColor: COLOURS[severity].text }],
    ]),
  ),
});

/** The drawing of diagnostics; nothing shows until some are set. */
export const diagnostics: Extension = [field, look];

/** For tests: what is drawn in a state, as `[from, to, class, message written out]`. */
export function drawn(state: {
  field: (f: typeof field) => DecorationSet;
}): [number, number, string, string?][] {
  const out: [number, number, string, string?][] = [];
  const cursor = state.field(field).iter();
  for (; cursor.value; cursor.next()) {
    const spec = cursor.value.spec as { class: string; attributes?: Record<string, string> };
    const lens = spec.attributes?.["data-lens"];
    out.push(lens === undefined ? [cursor.from, cursor.to, spec.class] : [cursor.from, cursor.to, spec.class, lens]);
  }
  return out;
}
