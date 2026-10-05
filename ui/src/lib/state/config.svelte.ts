/**
 * The config file, as the frontend sees it.
 *
 * The backend owns the file. This holds the latest copy it sent — on load,
 * and again whenever the file is saved or the Appearance dialog changes it —
 * so every part of the window reads its settings from one place.
 *
 * Field names are the file's own, snake case and all: what is written in
 * `config.toml` is what is read here, with no second spelling to keep in step.
 */
import { invoke } from "@tauri-apps/api/core";

export type Material = "none" | "acrylic";

export type Appearance = {
  /** Percent. 100 is opaque; the window only goes translucent below it. */
  background_opacity: number;
  material: Material;
  /** Percent. Zooms the interface and sizes the window to match. */
  scale: number;
};

export type EditorConfig = {
  font_family: string;
  font_size: number;
  line_height: number;
  tab_width: number;
  insert_spaces: boolean;
  detect_indentation: boolean;
  line_numbers: boolean;
  word_wrap: boolean;
};

export type LanguageOverride = { tab_width?: number; insert_spaces?: boolean };

export type Config = {
  appearance: Appearance;
  editor: EditorConfig;
  files: { exclude: string[] };
  theme: { syntax: string };
  /** By language name, lower-case. */
  language: Record<string, LanguageOverride>;
};

/** Mirrors the Rust defaults, for the frame before the backend has answered. */
export const DEFAULTS: Config = {
  appearance: { background_opacity: 100, material: "none", scale: 100 },
  editor: {
    font_family: "'Operator Mono', 'Geist Mono', Consolas, monospace",
    font_size: 14,
    line_height: 1.6,
    tab_width: 4,
    insert_spaces: true,
    detect_indentation: true,
    line_numbers: true,
    word_wrap: false,
  },
  files: { exclude: [".*"] },
  theme: { syntax: "catppuccin" },
  language: {},
};

export class ConfigStore {
  current = $state.raw<Config>(DEFAULTS);

  get appearance(): Appearance {
    return this.current.appearance;
  }

  /** 0–1, as CSS wants it. */
  get alpha(): number {
    return this.current.appearance.background_opacity / 100;
  }

  async load() {
    try {
      this.current = await invoke<Config>("config");
    } catch (e) {
      console.error("config failed", e);
    }
  }

  /**
   * Change some of the appearance. Returns once the backend has stored it.
   *
   * The backend is the authority on the stored value: it clamps opacity and
   * scale, so this adopts what comes back rather than what was sent.
   */
  async patchAppearance(change: Partial<Appearance>) {
    const next = { ...this.current.appearance, ...change };
    // Show it immediately: waiting for the round trip makes a dragged slider
    // feel like it is fighting back.
    this.current = { ...this.current, appearance: next };

    try {
      const stored = await invoke<Appearance>("set_appearance", { appearance: next });
      this.current = { ...this.current, appearance: stored };
    } catch (e) {
      console.error("set_appearance failed", e);
      // Whatever is actually stored is the truth; put it back.
      await this.load();
    }
  }

  async resetAppearance() {
    await this.patchAppearance({ ...DEFAULTS.appearance });
  }
}
