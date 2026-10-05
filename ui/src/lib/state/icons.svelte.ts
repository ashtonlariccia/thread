/**
 * The icon lookups, loaded after startup.
 *
 * The map is ~85 KB of JSON that nothing needs until a file is open, so it is
 * kept out of the startup payload and fetched once the window is up. Until it
 * lands every file wears the plain icon; rows re-render when it arrives.
 */
import { fileIconId, iconUrl, type FileIconMap } from "../icons";

let files = $state.raw<FileIconMap | null>(null);

export function loadIcons() {
  void import("../icons/files.json")
    .then((m) => (files = m.default as unknown as FileIconMap))
    .catch((e) => console.error("loading the icon map failed", e));
}

/** The URL of the icon for a file name. */
export function fileIcon(name: string): string {
  return iconUrl(files ? fileIconId(files, name) : "file");
}
