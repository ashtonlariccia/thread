/**
 * How the window looks: how solid it is, what is behind it, and how big.
 *
 * The backend is the authority on the stored value. It clamps opacity and
 * scale, so `patch` adopts what comes back rather than what was sent; a dialog
 * that kept showing 900% after the store clamped it to 200 would be lying
 * about what is saved.
 */
import { invoke } from "@tauri-apps/api/core";

export type Material = "none" | "acrylic";

export type Appearance = {
  /** Percent. 100 is opaque; the window only goes translucent below it. */
  backgroundOpacity: number;
  material: Material;
  /** Percent. Zooms the interface and sizes the window to match. */
  scale: number;
};

/** Mirrors the Rust defaults, for the frame before the backend has answered. */
export const DEFAULTS: Appearance = {
  backgroundOpacity: 100,
  material: "none",
  scale: 100,
};

export class AppearanceStore {
  current = $state<Appearance>({ ...DEFAULTS });

  /** 0–1, as CSS wants it. */
  get alpha(): number {
    return this.current.backgroundOpacity / 100;
  }

  async load() {
    try {
      this.current = await invoke<Appearance>("appearance");
    } catch (e) {
      console.error("appearance failed", e);
    }
  }

  /** Change some of it. Returns once the backend has stored and echoed it back. */
  async patch(change: Partial<Appearance>) {
    const next = { ...this.current, ...change };
    // Show it immediately: waiting for the round trip makes a dragged slider
    // feel like it is fighting back.
    this.current = next;

    try {
      this.current = await invoke<Appearance>("set_appearance", { appearance: next });
    } catch (e) {
      console.error("set_appearance failed", e);
      // Whatever is actually stored is the truth; put it back.
      await this.load();
    }
  }

  async reset() {
    await this.patch({ ...DEFAULTS });
  }
}
