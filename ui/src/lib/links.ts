/**
 * What in a line of terminal output can be followed: web addresses, and
 * paths to files, with the line and column a compiler puts after them.
 */

export type Link = {
  /** Where in the line it is: the first character, and one past the last. */
  start: number;
  end: number;
} & ({ url: string } | { path: string; line?: number; col?: number });

const URL = /https?:\/\/[^\s"'<>`]+/g;

/**
 * A path, and after it `:12`, `:12:5`, `(12)` or `(12,5)`. It has to end in
 * an extension, which is most of what tells a path from a word.
 */
const PATH =
  /((?:[A-Za-z]:[\\/]|~?\.{0,2}[\\/])?[\w.@+-]+(?:[\\/][\w.@+-]+)*\.[A-Za-z]\w{0,9})(?::(\d+)(?::(\d+))?|\((\d+)(?:,\s*(\d+))?\))?/g;

/** What a sentence ends with, which a link at the end of one does not include. */
const TRAILING = /[.,;:!?)\]}]+$/;

export function findLinks(text: string): Link[] {
  const links: Link[] = [];

  for (const found of text.matchAll(URL)) {
    const url = found[0].replace(TRAILING, "");
    links.push({ start: found.index, end: found.index + url.length, url });
  }
  const inUrl = (at: number) => links.some((link) => at >= link.start && at < link.end);

  for (const found of text.matchAll(PATH)) {
    const start = found.index;
    // Part of a longer word, or of an address already found.
    if (start > 0 && /[\w\\/.@+-]/.test(text[start - 1])) continue;
    if (inUrl(start)) continue;

    const path = found[1];
    const line = found[2] ?? found[4];
    const col = found[3] ?? found[5];
    // A bare `name.ext` is as likely a word in a sentence as a file: it
    // counts with a folder before it, or a line number after.
    if (!/[\\/]/.test(path) && line === undefined) continue;

    links.push({
      start,
      end: start + found[0].length,
      path,
      ...(line === undefined ? {} : { line: Number(line) }),
      ...(col === undefined ? {} : { col: Number(col) }),
    });
  }
  return links.sort((a, b) => a.start - b.start);
}

/**
 * Where a path from a terminal leads, given the folder the terminal was
 * started in. An absolute path is itself; anything else is from that folder.
 * (A shell that has since changed folder has not said so, and its relative
 * paths will miss.)
 */
export function resolveLink(path: string, cwd: string | null): string {
  if (/^([A-Za-z]:[\\/]|[\\/])/.test(path) || cwd === null) return path;
  const sep = cwd.includes("\\") ? "\\" : "/";
  const rest = path.replace(/^\.[\\/]/, "");
  return cwd.replace(/[\\/]$/, "") + sep + (sep === "\\" ? rest.replaceAll("/", "\\") : rest);
}
