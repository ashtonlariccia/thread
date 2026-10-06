/**
 * What a language server has to say about a file, drawn where it applies.
 *
 * Each complaint is underlined where it is, and its line is tinted and ends
 * in a dot of the same colour, with how many complaints the line has. Point
 * at the line and the dot opens into what they say: every one of them, in
 * full, wrapped to whatever room the window has to the right of the text.
 * After the VS Code extension Error Lens, less the part where a long message
 * runs off the side of a small window.
 *
 * None of it is text in the editor. The dot and the messages are drawn by
 * the line's own stylesheet, from attributes, so there is nothing there for
 * the cursor to be beside, for a selection to take in, or for a copy to pick
 * up; and the messages are laid over the lines below rather than among them,
 * so opening them moves nothing. (Which costs the dot while they are open:
 * a line has the one thing to draw them with.)
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

/** What a line's complaints say, written out: the most serious first, one to a line. */
export function lensText(all: Diagnostic[]): string {
  return [...all]
    .sort((a, b) => SEVERITIES.indexOf(a.severity) - SEVERITIES.indexOf(b.severity))
    .map((d) => (d.source ? `${d.message.trim()} (${d.source})` : d.message.trim()))
    .join("\n");
}

function build(doc: Text, diagnostics: Diagnostic[]): DecorationSet {
  type Line = { from: number; worst: Diagnostic; all: Diagnostic[] };
  const lines = new Map<number, Line>();
  const marks: { from: number; to: number; severity: Severity }[] = [];

  for (const diagnostic of diagnostics) {
    const from = Math.min(Math.max(0, diagnostic.from), doc.length);
    const line = doc.lineAt(from);
    let to = Math.min(Math.max(from, diagnostic.to), doc.length);
    // A complaint about a point is drawn under the character at it.
    if (to === from) to = Math.min(from + 1, line.to);
    if (to > from) marks.push({ from, to, severity: diagnostic.severity });

    const known = lines.get(line.from);
    if (!known) {
      lines.set(line.from, { from: line.from, worst: diagnostic, all: [diagnostic] });
    } else {
      known.all.push(diagnostic);
      const rank = (d: Diagnostic) => SEVERITIES.indexOf(d.severity);
      if (rank(diagnostic) < rank(known.worst)) known.worst = diagnostic;
    }
  }

  const ranges = [
    ...marks.map(({ from, to, severity }) =>
      Decoration.mark({ class: `cm-lens-range cm-lens-range-${severity}` }).range(from, to),
    ),
    ...[...lines.values()].map(({ from, worst, all }) =>
      Decoration.line({
        class: `cm-lens-line cm-lens-line-${worst.severity}`,
        attributes: { "data-lens": lensText(all), "data-lens-count": String(all.length) },
      }).range(from),
    ),
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

const COLOURS: Record<Severity, string> = {
  error: "var(--danger)",
  warning: "#f9e2af",
  info: "#89b4fa",
  hint: "var(--fg-dim)",
};

const look = EditorView.baseTheme({
  // The dot: how many, in the colour of the worst of them.
  ".cm-lens-line::after": {
    content: "attr(data-lens-count)",
    display: "inline-block",
    boxSizing: "border-box",
    minWidth: "1.5em",
    height: "1.5em",
    marginLeft: "2ch",
    padding: "0 0.4em",
    borderRadius: "999px",
    color: "var(--accent-ink)",
    fontSize: "0.7em",
    fontWeight: "700",
    lineHeight: "1.5em",
    textAlign: "center",
    verticalAlign: "middle",
  },
  // Pointed at, it is the messages, in a box hung under the line: taken out
  // of the flow, so it is as tall as it needs to be and nothing moves to
  // make room for it. It is as wide as what it says, up to the width of the
  // editor it is in, which is what wraps it to a small window or a narrow
  // pane. The pointer goes through it, so the lines it lies over can still
  // be pointed at, and it gives way to theirs.
  ".cm-scroller": { containerType: "inline-size" },
  ".cm-lens-line": { position: "relative" },
  ".cm-lens-line:hover::after": {
    content: "attr(data-lens)",
    position: "absolute",
    top: "100%",
    left: "2ch",
    zIndex: "5",
    width: "max-content",
    maxWidth: "calc(100cqw - 9ch)",
    minWidth: "0",
    height: "auto",
    margin: "0",
    padding: "1px 0.8em",
    borderRadius: "5px",
    border: "1px solid",
    backgroundColor: "var(--bg-menu)",
    boxShadow: "0 6px 18px #0008",
    fontSize: "0.9em",
    fontWeight: "400",
    lineHeight: "inherit",
    textAlign: "left",
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
      [`.cm-lens-line-${severity}::after`, { backgroundColor: COLOURS[severity] }],
      [`.cm-lens-line-${severity}:hover::after`, { color: COLOURS[severity] }],
      [`.cm-lens-range-${severity}`, { textDecorationColor: COLOURS[severity] }],
      [
        `.cm-lens-line-${severity}`,
        { backgroundColor: `color-mix(in srgb, ${COLOURS[severity]} 9%, transparent)` },
      ],
    ]),
  ),
});

/** The drawing of diagnostics; nothing shows until some are set. */
export const diagnostics: Extension = [field, look];

/** For tests: what is drawn in a state, as `[from, to, class, messages written out]`. */
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
