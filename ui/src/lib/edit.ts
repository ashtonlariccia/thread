/**
 * The Edit menu's commands, aimed at whatever text field has focus.
 *
 * `execCommand` is deprecated on the web at large, but inside a webview it is
 * still the only way to drive a field's *own* undo stack and selection from a
 * menu. The editor will replace these with its own commands when it exists.
 */
import { readText } from "@tauri-apps/plugin-clipboard-manager";

export type EditCommand = "undo" | "redo" | "cut" | "copy" | "paste" | "selectAll";

/** Whether focus is somewhere the commands can act on. */
export function canEdit(): boolean {
  const el = document.activeElement;
  return el instanceof HTMLElement && (el.isContentEditable || el.matches("input, textarea"));
}

export async function runEdit(command: EditCommand) {
  if (command !== "paste") {
    document.execCommand(command);
    return;
  }

  // The webview refuses `execCommand("paste")`, so the text comes from the
  // backend and goes in as typing — which keeps it on the field's undo stack.
  try {
    document.execCommand("insertText", false, await readText());
  } catch (e) {
    console.error("paste failed", e);
  }
}
