/** The file name at the end of a path, whichever slash it uses. */
export function baseName(path: string): string {
  return path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1);
}

/** The folder a path is in: everything before its last component. */
export function dirName(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (cut < 0) return "";
  // A root keeps its slash: `C:\file` is in `C:\`, not in `C:`, which means
  // something else entirely, and `/file` is in `/`.
  const root = cut === 0 || (cut === 2 && path[1] === ":");
  return path.slice(0, root ? cut + 1 : cut);
}

/**
 * Whether a path is a remote's: they are all absolute and POSIX, so they
 * start with a slash, which no path on this machine does.
 */
const posix = (path: string) => path.startsWith("/");

/**
 * A path as it is compared. This machine's are case-insensitive and take
 * either slash; a remote's are exactly what they say.
 */
const fold = (path: string) => (posix(path) ? path : path.replaceAll("\\", "/").toLowerCase());

/**
 * The folder names leading from `root` down to `path`, then its own name —
 * or null if `path` is not inside `root`.
 *
 * Compared the way `samePath` compares, so a root spelled `c:/src` still
 * contains `C:\src\lib\a.rs`.
 */
export function segmentsBelow(root: string, path: string): string[] | null {
  const base = fold(root).replace(/\/+$/, "") + "/";
  if (!fold(path).startsWith(base)) return null;

  // On a remote a backslash is a character in a name, not a separator.
  const rest = path.slice(base.length).split(posix(path) ? /\/+/ : /[\\/]+/).filter(Boolean);
  return rest.length > 0 ? rest : null;
}

/**
 * A path reduced to what identifies the file, for use as a lookup key.
 *
 * Paths on this machine: case-insensitive, and either slash. A remote's are
 * left as they are, case and all. Not a full canonical
 * form — it will not see through a symlink or `..` — but two spellings of one
 * file from a dialog, the tree and the command line all come out the same.
 */
export function pathKey(path: string): string {
  return fold(path);
}

/** Whether two paths name the same file, by [`pathKey`]. */
export function samePath(a: string, b: string): boolean {
  return pathKey(a) === pathKey(b);
}
