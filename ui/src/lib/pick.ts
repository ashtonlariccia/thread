/**
 * Asking which file or folder: the system's dialogs here, and a stand-in for
 * them on a remote, where the system has nothing to browse.
 *
 * The window installs the stand-in while it is connected. Whoever asks does
 * not need to know which answered.
 */
import { open, save } from "@tauri-apps/plugin-dialog";

export type RemotePicker = {
  files: () => Promise<string[] | null>;
  folder: () => Promise<string | null>;
  /** `suggested` is the path or the bare name the file has now. */
  save: (suggested: string) => Promise<string | null>;
};

let remote: RemotePicker | null = null;

/** Set while the window is on a remote; null puts the system's dialogs back. */
export function setRemotePicker(picker: RemotePicker | null) {
  remote = picker;
}

/** File → Open File. Null if nothing was chosen. */
export async function pickFiles(): Promise<string[] | null> {
  if (remote) return remote.files();
  const picked = await open({ title: "Open File", multiple: true });
  if (!picked) return null;
  return Array.isArray(picked) ? picked : [picked];
}

/** File → Open Folder. */
export async function pickFolder(): Promise<string | null> {
  if (remote) return remote.folder();
  const picked = await open({ title: "Open Folder", directory: true });
  return typeof picked === "string" ? picked : null;
}

/** File → Save As. */
export async function pickSave(suggested: string): Promise<string | null> {
  if (remote) return remote.save(suggested);
  return save({ title: "Save As", defaultPath: suggested });
}
