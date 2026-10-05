/**
 * How a file is indented: worked out from its text, then settled against the
 * config.
 */

/** What Tab inserts, and how wide a tab character is drawn. */
export type Indent = { spaces: boolean; width: number };

/** What a file's own text says. `width` is null when it only says "tabs". */
export type Detected = { spaces: boolean; width: number | null };

/** The config's say: `[editor]`, with any `[language.x]` override on top. */
export type IndentPreference = {
  tab_width: number;
  insert_spaces: boolean;
  detect_indentation: boolean;
};

/** Enough to be sure of a file's habits without reading all of a huge one. */
const SAMPLE_LINES = 2000;

/**
 * Work out a file's indentation from how it is already indented.
 *
 * Tabs or spaces is a vote between lines that start with one or the other.
 * The width of a space indent is the most common *step* from one line's
 * indentation to the next — the change, not the amount, so a file nested four
 * deep at two spaces is not mistaken for eight.
 *
 * Null when nothing in the file is indented: there is nothing to go on.
 */
export function detectIndent(text: string): Detected | null {
  let tabs = 0;
  let spaces = 0;
  const steps = new Map<number, number>();
  let previous = 0;

  let start = 0;
  for (let n = 0; n < SAMPLE_LINES && start <= text.length; n++) {
    let end = text.indexOf("\n", start);
    if (end === -1) end = text.length;
    const line = text.slice(start, end);
    start = end + 1;

    const body = line.trimStart();
    // Blank lines say nothing. Nor do the ` * ` lines of a block comment,
    // which sit one space in whatever the file's indentation is.
    if (body === "" || body.startsWith("*")) continue;

    const lead = line.length - body.length;
    if (line[0] === "\t") {
      tabs++;
      previous = 0;
      continue;
    }
    if (lead > 0) spaces++;

    const step = Math.abs(lead - previous);
    if (step > 1 && step <= 8) steps.set(step, (steps.get(step) ?? 0) + 1);
    previous = lead;
  }

  if (tabs === 0 && spaces === 0) return null;
  if (tabs > spaces) return { spaces: false, width: null };

  let width: number | null = null;
  let most = 0;
  for (const [step, count] of steps) {
    // On a tie the smaller step wins: a larger one is that step taken twice.
    if (count > most || (count === most && width !== null && step < width)) {
      width = step;
      most = count;
    }
  }
  return { spaces: true, width };
}

/**
 * The indentation a file gets: the config's, unless detection is on and the
 * file has habits of its own.
 *
 * A file indented with tabs keeps the configured width — the file says *that*
 * it uses tabs, not how wide to draw them.
 */
export function resolveIndent(preference: IndentPreference, detected: Detected | null): Indent {
  const indent: Indent = { spaces: preference.insert_spaces, width: preference.tab_width };
  if (preference.detect_indentation && detected) {
    indent.spaces = detected.spaces;
    if (detected.spaces && detected.width !== null) indent.width = detected.width;
  }
  return indent;
}

/** As the bottom bar shows it. */
export function indentLabel(indent: Indent): string {
  return `${indent.spaces ? "Spaces" : "Tabs"}: ${indent.width}`;
}
