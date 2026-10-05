/**
 * Which icon a file wears.
 *
 * The pack is Material Icon Theme, imported from the VS Code extension by
 * `scripts/import-icons.mjs`: the SVGs land in `ui/public/icons` and the
 * lookups in `icons/files.json` and `icons/folders.json`.
 */

/** The shape of `icons/files.json`. Keys are lower-case. */
export type FileIconMap = {
  /** The icon for a file nothing else matches. */
  file: string;
  extensions: Record<string, string>;
  names: Record<string, string>;
};

/**
 * The icon id for a file name.
 *
 * A whole-name match wins (`package.json`, `Dockerfile`), then the longest
 * extension the pack knows — so `types.d.ts` is a declaration file rather than
 * plain TypeScript, and `.gitignore` is matched as the extension `gitignore`.
 */
export function fileIconId(map: FileIconMap, name: string): string {
  const lower = name.toLowerCase();
  const named = map.names[lower];
  if (named) return named;

  for (let dot = lower.indexOf("."); dot !== -1; dot = lower.indexOf(".", dot + 1)) {
    const icon = map.extensions[lower.slice(dot + 1)];
    if (icon) return icon;
  }
  return map.file;
}

/** The shape of `icons/folders.json`. Keys are lower-case. */
export type FolderIconMap = {
  folder: string;
  folderOpen: string;
  /** The folder a project was opened at. */
  root: string;
  rootOpen: string;
  names: Record<string, string>;
  namesOpen: Record<string, string>;
};

/**
 * The icon id for a folder.
 *
 * The root is always the root icon, whatever it is called: it marks where the
 * project starts, which matters more than that the folder happens to be `src`.
 */
export function folderIconId(
  map: FolderIconMap,
  name: string,
  open: boolean,
  root = false,
): string {
  if (root) return open ? map.rootOpen : map.root;
  const lower = name.toLowerCase();
  return open ? (map.namesOpen[lower] ?? map.folderOpen) : (map.names[lower] ?? map.folder);
}

export function iconUrl(id: string): string {
  return `/icons/${id}.svg`;
}
