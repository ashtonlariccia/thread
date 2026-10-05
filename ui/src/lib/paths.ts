/** The file name at the end of a path, whichever slash it uses. */
export function baseName(path: string): string {
  return path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1);
}

/** The folder a path is in: everything before its last component. */
export function dirName(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  // `C:\file` is in `C:\`, not in `C:`, which means something else entirely.
  return cut <= 2 ? path.slice(0, cut + 1) : path.slice(0, cut);
}

/**
 * The folder names leading from `root` down to `path`, then its own name —
 * or null if `path` is not inside `root`.
 *
 * Compared the way `samePath` compares, so a root spelled `c:/src` still
 * contains `C:\src\lib\a.rs`.
 */
export function segmentsBelow(root: string, path: string): string[] | null {
  const fold = (p: string) => p.replaceAll("\\", "/").toLowerCase();
  const base = fold(root).replace(/\/+$/, "") + "/";
  if (!fold(path).startsWith(base)) return null;

  const rest = path.slice(base.length).split(/[\\/]+/).filter(Boolean);
  return rest.length > 0 ? rest : null;
}

/**
 * A path reduced to what identifies the file, for use as a lookup key.
 *
 * Windows paths: case-insensitive, and either slash. Not a full canonical
 * form — it will not see through a symlink or `..` — but two spellings of one
 * file from a dialog, the tree and the command line all come out the same.
 */
export function pathKey(path: string): string {
  return path.replaceAll("\\", "/").toLowerCase();
}

/** Whether two paths name the same file, by [`pathKey`]. */
export function samePath(a: string, b: string): boolean {
  return pathKey(a) === pathKey(b);
}
