/**
 * The Edit menu's commands, aimed at whatever has focus.
 *
 * The editor answers for itself when it has focus. Anywhere else — a field in
 * a dialog — falls back to `execCommand`, which is deprecated on the web at
 * large but is still the only way to drive a plain field's *own* undo stack
 * and selection from a menu.
 */
import { readText } from "@tauri-apps/plugin-clipboard-manager";

import type { EditorHost } from "./editor";

export type EditCommand = "undo" | "redo" | "cut" | "copy" | "paste" | "selectAll";

let editor: EditorHost | null = null;

/** Tell the menu which editor to drive. */
export function setEditor(host: EditorHost | null) {
  editor = host;
}

/** Whether focus is somewhere the commands can act on. */
export function canEdit(): boolean {
  const el = document.activeElement;
  return el instanceof HTMLElement && (el.isContentEditable || el.matches("input, textarea"));
}

export async function runEdit(command: EditCommand) {
  const target = editor?.hasFocus() ? editor : null;

  if (command === "paste") {
    // The webview refuses `execCommand("paste")`, so the text comes from the
    // backend and goes in as typing — which keeps it on the undo stack.
    try {
      const text = await readText();
      if (target) target.insert(text);
      else document.execCommand("insertText", false, text);
    } catch (e) {
      console.error("paste failed", e);
    }
    return;
  }

  // Cut and copy go through the browser either way: the editor handles the
  // clipboard events they raise.
  if (target && command !== "cut" && command !== "copy") target.run(command);
  else document.execCommand(command);
}
