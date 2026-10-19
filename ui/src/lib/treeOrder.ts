/**
 * How the file tree orders a folder's entries, beyond the listing's own order
 * (folders first, each lot by name).
 */

/** The part of a name before `ext`, lower-case, or null if it does not end in it. */
function stem(name: string, ext: string): string | null {
  const lower = name.toLowerCase();
  return lower.endsWith(ext) && lower.length > ext.length ? lower.slice(0, -ext.length) : null;
}

/**
 * The entries with each `foo.h` moved to just above its `foo.c`: by name the
 * source comes first, and the header is what is read first. A header with no
 * source of the same name stays where it was.
 */
export function headersFirst<T extends { name: string; dir: boolean }>(entries: T[]): T[] {
  const headers = new Map<string, T>();
  const sources = new Set<string>();
  for (const entry of entries) {
    if (entry.dir) continue;
    const h = stem(entry.name, ".h");
    if (h !== null) headers.set(h, entry);
    const c = stem(entry.name, ".c");
    if (c !== null) sources.add(c);
  }

  const out: T[] = [];
  for (const entry of entries) {
    if (!entry.dir) {
      const h = stem(entry.name, ".h");
      // Out with its source instead.
      if (h !== null && sources.has(h)) continue;
      const c = stem(entry.name, ".c");
      const header = c === null ? undefined : headers.get(c);
      if (header) out.push(header);
    }
    out.push(entry);
  }
  return out;
}
