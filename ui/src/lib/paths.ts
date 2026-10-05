/** The file name at the end of a path, whichever slash it uses. */
export function baseName(path: string): string {
  return path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1);
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
 * Whether two paths name the same file.
 *
 * Windows paths: case-insensitive, and either slash. Not a full canonical
 * comparison — it will not see through a symlink or `..` — but it is what
 * stops the same file being picked twice from a dialog opening twice.
 */
export function samePath(a: string, b: string): boolean {
  const fold = (p: string) => p.replaceAll("\\", "/").toLowerCase();
  return fold(a) === fold(b);
}
