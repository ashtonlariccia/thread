/**
 * What a language server has to say about a file, drawn where it applies.
 *
 * Each complaint is underlined where it is, and written out at the end of
 * the line it starts on, with the line tinted to match: the reading of a
 * problem without hovering over it or opening a panel, after the VS Code
 * extension Error Lens. One message to a line, the most serious there, cut
 * short if it is long; all of it is in the line's tooltip.
 *
 * The message is not text in the editor. It is drawn by the line's own
 * stylesheet, from an attribute, so there is nothing there for the cursor to
 * be beside, for a selection to take in, or for a copy to pick up.
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

/** As much of a message as is written at the end of a line. */
const LENS_CHARS = 72;

/** A message as it is written out: its first line, and not too much of that. */
export function lensText(message: string, others: number): string {
  const first = message.split("\n")[0].trim();
  const cut = first.length > LENS_CHARS ? `${first.slice(0, LENS_CHARS).trimEnd()}…` : first;
  return others > 0 ? `${cut}  (+${others})` : cut;
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
        attributes: {
          "data-lens": lensText(worst.message, all.length - 1),
          // Everything said about the line, and all of each.
          title: all.map((d) => (d.source ? `${d.message} — ${d.source}` : d.message)).join("\n\n"),
        },
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
  ".cm-lens-line::after": {
    content: "attr(data-lens)",
    marginLeft: "3ch",
    fontStyle: "italic",
    opacity: "0.85",
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
      [`.cm-lens-line-${severity}::after`, { color: COLOURS[severity] }],
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
