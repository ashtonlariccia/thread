/**
 * What a language server has to say about a file, drawn where it applies.
 *
 * A line with problems is tinted, and after the end of it is a dot for each
 * of them, in the colour of its severity. On the line the cursor is on, the
 * dots give way to the problem written out, as the VS Code extension Error
 * Lens writes it at its defaults: four characters on, the most serious on
 * the line, on one line however long it is, in that extension's colours.
 * Hints are underlined, and get neither.
 *
 * None of it is text in the editor. It is drawn by the line's own
 * stylesheet, from attributes, so there is nothing there for the cursor to
 * be beside, for a selection to take in, or for a copy to pick up; and it is
 * out of the line's flow, so a long message makes the line no wider and is
 * simply cut off where the editor ends.
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

/** The most dots a line is given: past this it is "a lot" either way. */
const MAX_DOTS = 8;

/**
 * The dots after a line, one for each problem and the most serious first, as
 * the two things a stylesheet draws them from: the colour of the first, and
 * the rest as shadows of it, each a step further along.
 */
export function lensDots(all: Diagnostic[]): { first: string; rest: string } {
  const colours = [...all]
    .sort((a, b) => rank(a) - rank(b))
    .slice(0, MAX_DOTS)
    .map((d) => COLOURS[d.severity].text);
  const rest = colours.slice(1).map((colour, index) => `${(index + 1) * DOT_STEP}em 0 0 ${colour}`);
  return { first: colours[0], rest: rest.length > 0 ? rest.join(", ") : "none" };
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
      const dots = lensDots(all);
      return Decoration.line({
        class: `cm-lens-line cm-lens-line-${worst.severity}`,
        attributes: {
          "data-lens": lensText(worst.message),
          style: `--lens-dot: ${dots.first}; --lens-dots: ${dots.rest}`,
        },
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

/** A dot's width, and how far each is from the one before, in ems. */
const DOT_SIZE = 0.56;
const DOT_STEP = 0.95;

const look = EditorView.baseTheme({
  // After the line, and out of its flow though it sits where it would have
  // in it: so that nothing drawn here makes the line any wider, and what
  // does not fit is cut off where the line ends rather than giving the
  // editor somewhere to scroll sideways to.
  ".cm-lens-line": { position: "relative", overflowX: "clip" },
  // The dots. One is drawn, and the others are its shadows.
  ".cm-lens-line::after": {
    content: '""',
    position: "absolute",
    width: `${DOT_SIZE}em`,
    height: `${DOT_SIZE}em`,
    marginLeft: "4ch",
    // Halfway down the row of text, whatever the line height is.
    marginTop: `calc((1lh - ${DOT_SIZE}em) / 2)`,
    borderRadius: "50%",
    backgroundColor: "var(--lens-dot)",
    boxShadow: "var(--lens-dots)",
  },
  // On the cursor's line, in the editor being typed into: the message.
  "&.cm-focused .cm-activeLine.cm-lens-line::after": {
    content: "attr(data-lens)",
    width: "auto",
    height: "auto",
    marginTop: "0",
    borderRadius: "0",
    backgroundColor: "transparent",
    boxShadow: "none",
    whiteSpace: "pre",
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
