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
  relative_line_numbers: boolean;
  word_wrap: boolean;
  smooth_caret: boolean;
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
    relative_line_numbers: false,
    word_wrap: false,
    smooth_caret: true,
  },
  files: { exclude: [".*"] },
  theme: { syntax: "catppuccin" },
  language: {},
};

/** The sections whose keys are plain values, settable one at a time. */
type Settable = "appearance" | "editor" | "files" | "theme";

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
   * Change one setting. The backend writes it into the config file — the same
   * change as editing that key by hand — and applies it everywhere.
   *
   * The backend is the authority on the stored value: it clamps, so this
   * adopts what comes back rather than what was sent.
   */
  async set<S extends Settable, K extends keyof Config[S]>(section: S, key: K, value: Config[S][K]) {
    // Show it immediately: waiting for the round trip makes a dragged slider
    // feel like it is fighting back.
    this.current = { ...this.current, [section]: { ...this.current[section], [key]: value } };

    try {
      this.current = await invoke<Config>("set_config", { section, key, value });
    } catch (e) {
      console.error("set_config failed", e);
      // Whatever is actually stored is the truth; put it back.
      await this.load();
    }
  }

  /** Put every setting the dialog shows back to its default. */
  async reset() {
    for (const section of ["appearance", "editor", "files", "theme"] as const) {
      const defaults = DEFAULTS[section] as Record<string, unknown>;
      const current = this.current[section] as Record<string, unknown>;
      for (const key of Object.keys(defaults)) {
        // Only what differs, so a reset does not rewrite the whole file.
        if (JSON.stringify(current[key]) === JSON.stringify(defaults[key])) continue;
        await this.set(section, key as never, defaults[key] as never);
      }
    }
  }
}
