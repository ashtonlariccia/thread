/** The file name at the end of a path, whichever slash it uses. */
export function baseName(path: string): string {
  return path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1);
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
