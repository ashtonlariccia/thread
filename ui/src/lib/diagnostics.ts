/**
 * What a language server has to say about a file, drawn where it applies.
 *
 * Each complaint is underlined where it is, and written out at the end of
 * the line it starts on, with the line tinted to match: the reading of a
 * problem without hovering over it or opening a panel, after the VS Code
 * extension Error Lens. One message to a line, the most serious there.
 *
 * This only draws. What the complaints are is whoever sets them's business.
 */
import { StateEffect, StateField, type Extension, type Text } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";

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

class LensWidget extends WidgetType {
  constructor(
    readonly severity: Severity,
    readonly text: string,
    /** Everything said about the line, for the tooltip. */
    readonly full: string,
  ) {
    super();
  }

  override eq(other: LensWidget) {
    return this.severity === other.severity && this.text === other.text && this.full === other.full;
  }

  override toDOM() {
    const el = document.createElement("span");
    el.className = `cm-lens cm-lens-${this.severity}`;
    el.textContent = this.text;
    el.title = this.full;
    return el;
  }

  // Reading a message is not placing the cursor.
  override ignoreEvent() {
    return true;
  }
}

function build(doc: Text, diagnostics: Diagnostic[]): DecorationSet {
  type Line = { from: number; to: number; worst: Diagnostic; all: Diagnostic[] };
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
      lines.set(line.from, { from: line.from, to: line.to, worst: diagnostic, all: [diagnostic] });
    } else {
      known.all.push(diagnostic);
      const rank = (d: Diagnostic) => SEVERITIES.indexOf(d.severity);
      if (rank(diagnostic) < rank(known.worst)) known.worst = diagnostic;
    }
  }

  // Three kinds of range, each already in order; the set wants them merged.
  const ranges = [
    ...marks.map(({ from, to, severity }) =>
      Decoration.mark({ class: `cm-lens-range cm-lens-range-${severity}` }).range(from, to),
    ),
    ...[...lines.values()].flatMap((line) => {
      const { worst, all } = line;
      const more = all.length > 1 ? `  (+${all.length - 1})` : "";
      const full = all
        .map((d) => (d.source ? `${d.message} — ${d.source}` : d.message))
        .join("\n\n");
      return [
        Decoration.line({ class: `cm-lens-line cm-lens-line-${worst.severity}` }).range(line.from),
        Decoration.widget({
          // The first line of it: the rest is in the tooltip.
          widget: new LensWidget(worst.severity, worst.message.split("\n")[0] + more, full),
          side: 1,
        }).range(line.to),
      ];
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
    // Until the server has read the edit and said so, they stay with the
    // text they were about.
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
  ".cm-lens": {
    marginLeft: "3ch",
    fontStyle: "italic",
    opacity: "0.85",
    // Not text of the file: not selected with it, and not copied.
    userSelect: "none",
    WebkitUserSelect: "none",
    cursor: "default",
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
      [`.cm-lens-${severity}`, { color: COLOURS[severity] }],
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

/** For tests: what is drawn in a state, as `[from, to, class or text]`. */
export function drawn(state: { field: (f: typeof field) => DecorationSet }): [number, number, string][] {
  const out: [number, number, string][] = [];
  const cursor = state.field(field).iter();
  for (; cursor.value; cursor.next()) {
    const spec = cursor.value.spec as { class?: string; widget?: LensWidget };
    out.push([cursor.from, cursor.to, spec.class ?? spec.widget?.text ?? ""]);
  }
  return out;
}
