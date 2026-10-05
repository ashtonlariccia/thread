/** The pinned strip along the bottom of the window. */
import { invoke } from "@tauri-apps/api/core";

import { samePin } from "../pins";
import type { Pin } from "../types";

export class Pins {
  list = $state<Pin[]>([]);

  async refresh() {
    try {
      this.list = await invoke<Pin[]>("pins");
    } catch (e) {
      console.error("pins failed", e);
    }
  }

  has(pin: Pin | null): boolean {
    return pin !== null && this.list.some((p) => samePin(p, pin));
  }

  async add(pin: Pin) {
    try {
      this.list = await invoke<Pin[]>("add_pin", { pin });
    } catch (e) {
      console.error("add_pin failed", e);
    }
  }

  async remove(pin: Pin) {
    try {
      this.list = await invoke<Pin[]>("remove_pin", { kind: pin.kind, target: pin.target });
    } catch (e) {
      console.error("remove_pin failed", e);
    }
  }

  /** Dragged (or Ctrl+Arrowed) to a new position on the strip. */
  async move(pin: Pin, index: number) {
    // Optimistic: the strip has already shown the drop, and waiting for the
    // round trip would snap it back for a frame first.
    const without = this.list.filter((p) => !samePin(p, pin));
    const moved = this.list.find((p) => samePin(p, pin));
    if (moved) this.list = [...without.slice(0, index), moved, ...without.slice(index)];

    try {
      this.list = await invoke<Pin[]>("move_pin", { kind: pin.kind, target: pin.target, index });
    } catch (e) {
      console.error("move_pin failed", e);
      // Whatever the backend actually holds is the truth; put it back.
      await this.refresh();
    }
  }
}
