/**
 * The icon lookups, loaded after startup.
 *
 * The map is ~85 KB of JSON that nothing needs until a file is open, so it is
 * kept out of the startup payload and fetched once the window is up. Until it
 * lands every file wears the plain icon; rows re-render when it arrives.
 */
import {
  fileIconId,
  folderIconId,
  iconUrl,
  type FileIconMap,
  type FolderIconMap,
} from "../icons";

let files = $state.raw<FileIconMap | null>(null);
let folders = $state.raw<FolderIconMap | null>(null);
let foldersRequested = false;

export function loadIcons() {
  void import("../icons/files.json")
    .then((m) => (files = m.default as unknown as FileIconMap))
    .catch((e) => console.error("loading the icon map failed", e));
}

/**
 * Fetch the folder lookups. Asked for by the tree the first time a folder is
 * opened: it is three times the size of the file map, and a window that only
 * ever edits single files never needs it.
 */
export function loadFolderIcons() {
  if (foldersRequested) return;
  foldersRequested = true;
  void import("../icons/folders.json")
    .then((m) => (folders = m.default as unknown as FolderIconMap))
    .catch((e) => {
      foldersRequested = false;
      console.error("loading the folder icon map failed", e);
    });
}

/** The URL of the icon for a folder. */
export function folderIcon(name: string, open: boolean, root = false): string {
  if (folders) return iconUrl(folderIconId(folders, name, open, root));
  return iconUrl(open ? "folder-open" : "folder");
}

/** The URL of the icon for a file name. */
export function fileIcon(name: string): string {
  return iconUrl(files ? fileIconId(files, name) : "file");
}
