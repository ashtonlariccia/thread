<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  import Dialog from "./Dialog.svelte";
  import type { ConfigStore, EditorConfig } from "./state/config.svelte";
  import { THEME_NAMES } from "./themes";

  type Props = {
    open: boolean;
    config: ConfigStore;
    onclose: () => void;
  };

  let { open, config, onclose }: Props = $props();

  // Every control here is one key in the config file. Changing it writes that
  // key; editing the key by hand and saving moves the control. Neither is the
  // "real" one.
  const appearance = $derived(config.current.appearance);
  const editor = $derived(config.current.editor);

  /** The whole config as a file: these settings and the ones with no control here. */
  function openConfig() {
    onclose();
    invoke("open_config").catch((e) => console.error("open_config failed", e));
  }

  /**
   * A typed number, committed on `change` rather than `input`.
   *
   * Half-typed numbers are real values to the backend: 1 and 12 on the way to
   * 125 would each be clamped and applied, and for the scale that means
   * resizing the window on every keystroke.
   */
  async function commitNumber(event: Event, current: number, apply: (value: number) => Promise<void>) {
    const input = event.currentTarget as HTMLInputElement;
    const value = Number(input.value);
    if (input.value.trim() === "" || !Number.isFinite(value)) {
      input.value = String(current);
      return;
    }
    await apply(value);
  }

  /**
   * After a commit, show what was stored. The store may have clamped the
   * value back to what it already was, in which case nothing re-renders and
   * the box would go on showing what was typed.
   */
  function resync(event: Event, stored: () => number) {
    (event.currentTarget as HTMLInputElement).value = String(stored());
  }

  function numberField<K extends "font_size" | "line_height" | "tab_width">(key: K) {
    return async (event: Event) => {
      const target = event.currentTarget;
      await commitNumber(event, editor[key], (value) => config.set("editor", key, value));
      resync({ currentTarget: target } as Event, () => config.current.editor[key]);
    };
  }

  async function onScaleChange(event: Event) {
    const target = event.currentTarget;
    await commitNumber(event, appearance.scale, (value) =>
      config.set("appearance", "scale", Math.round(value)),
    );
    resync({ currentTarget: target } as Event, () => config.current.appearance.scale);
  }

  function onFontChange(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    // An empty font list is not a font; the backend would put the default
    // back, so do not pretend the box was cleared.
    if (input.value.trim() === "") input.value = editor.font_family;
    else void config.set("editor", "font_family", input.value.trim());
  }

  function onExcludeChange(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const patterns = input.value
      .split(",")
      .map((pattern) => pattern.trim())
      .filter(Boolean);
    void config.set("files", "exclude", patterns);
  }

  type Toggle = {
    key: keyof {
      [K in keyof EditorConfig as EditorConfig[K] extends boolean ? K : never]: true;
    };
    label: string;
    /** Greyed out while this is false: the setting has nothing to act on. */
    needs?: () => boolean;
  };

  const TOGGLES: Toggle[] = [
    { key: "insert_spaces", label: "Indent with spaces" },
    { key: "detect_indentation", label: "Detect indentation from each file" },
    { key: "line_numbers", label: "Line numbers" },
    {
      key: "relative_line_numbers",
      label: "Relative line numbers",
      needs: () => editor.line_numbers,
    },
    { key: "word_wrap", label: "Word wrap" },
    { key: "smooth_caret", label: "Smooth caret" },
  ];
</script>

<Dialog {open} title="Appearance" width={460} scrolls {onclose}>
  <div class="settings">
    <h3>Window</h3>
    <div class="grid">
      <label for="ap-opacity">Background</label>
      <div class="row">
        <!-- Floor is 20%, matching the backend's clamp: a window you can't
             see is a state you can't get out of from inside it. -->
        <input
          id="ap-opacity"
          type="range"
          min="20"
          max="100"
          step="1"
          value={appearance.background_opacity}
          oninput={(e) =>
            void config.set("appearance", "background_opacity", Number(e.currentTarget.value))}
        />
        <span class="unit pct">{appearance.background_opacity}%</span>
      </div>

      <span class="label-ish">Material</span>
      <label class="check">
        <input
          type="checkbox"
          checked={appearance.material === "acrylic"}
          onchange={(e) =>
            void config.set("appearance", "material", e.currentTarget.checked ? "acrylic" : "none")}
        />
        Acrylic — frost whatever is behind the window
      </label>

      <label for="ap-scale">Scale</label>
      <div class="row">
        <input
          id="ap-scale"
          type="number"
          min="50"
          max="200"
          step="5"
          value={appearance.scale}
          onchange={onScaleChange}
        />
        <span class="unit">%</span>
      </div>
    </div>

    <h3>Editor</h3>
    <div class="grid">
      <label for="ap-font">Font</label>
      <!-- A CSS font list, so it can name fallbacks: the first one installed
           is the one used. -->
      <input
        id="ap-font"
        type="text"
        spellcheck="false"
        value={editor.font_family}
        onchange={onFontChange}
      />

      <label for="ap-font-size">Size</label>
      <div class="row">
        <input
          id="ap-font-size"
          type="number"
          min="6"
          max="72"
          step="1"
          value={editor.font_size}
          onchange={numberField("font_size")}
        />
        <span class="unit">px</span>
      </div>

      <label for="ap-line-height">Line height</label>
      <div class="row">
        <input
          id="ap-line-height"
          type="number"
          min="1"
          max="3"
          step="0.1"
          value={editor.line_height}
          onchange={numberField("line_height")}
        />
        <span class="unit">× the font size</span>
      </div>

      <label for="ap-tab-width">Tab width</label>
      <div class="row">
        <input
          id="ap-tab-width"
          type="number"
          min="1"
          max="16"
          step="1"
          value={editor.tab_width}
          onchange={numberField("tab_width")}
        />
        <span class="unit">columns</span>
      </div>

      <span class="label-ish top">Options</span>
      <div class="checks">
        {#each TOGGLES as toggle (toggle.key)}
          {@const usable = toggle.needs?.() ?? true}
          <label class="check" class:off={!usable}>
            <input
              type="checkbox"
              checked={editor[toggle.key]}
              disabled={!usable}
              onchange={(e) => void config.set("editor", toggle.key, e.currentTarget.checked)}
            />
            {toggle.label}
          </label>
        {/each}
      </div>
    </div>

    <h3>Files</h3>
    <div class="grid">
      <label for="ap-exclude">Hide in tree</label>
      <input
        id="ap-exclude"
        type="text"
        spellcheck="false"
        placeholder="nothing hidden"
        value={config.current.files.exclude.join(", ")}
        onchange={onExcludeChange}
      />
    </div>
    <p class="hint">
      Names, separated by commas. <code>*</code> stands for anything, so <code>.*</code> is every
      dotfile.
    </p>

    <h3>Theme</h3>
    <div class="grid">
      <label for="ap-syntax">Syntax colours</label>
      <select
        id="ap-syntax"
        value={config.current.theme.syntax}
        onchange={(e) => void config.set("theme", "syntax", e.currentTarget.value)}
      >
        {#each THEME_NAMES as name (name)}
          <option value={name}>{name}</option>
        {/each}
        <!-- A name typed into the file that is not a theme: shown, so the box
             does not claim a theme that is not the one in the file. -->
        {#if !THEME_NAMES.includes(config.current.theme.syntax)}
          <option value={config.current.theme.syntax}>{config.current.theme.syntax} (unknown)</option>
        {/if}
      </select>
    </div>
  </div>

  <p class="dlg-note">
    Each setting is a key in the config file and is saved as you change it. Per-language
    overrides are only in the file.
  </p>

  <div class="dlg-actions">
    <button class="btn ghost config" onclick={openConfig}>Open Config File</button>
    <button class="btn ghost" onclick={() => void config.reset()}>Reset</button>
    <button class="btn primary" onclick={onclose}>Done</button>
  </div>
</Dialog>

<style>
  /* The part that scrolls: the dialog has more settings than a small window
     has height, and the buttons below have to stay reachable. */
  .settings {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    /* Room for the scrollbar, so it does not sit on the controls. */
    padding-right: 0.5rem;
    margin-right: -0.5rem;
  }

  h3 {
    margin: 1rem 0 0.5rem;
    color: var(--fg-faint);
    font-size: 0.68rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  h3:first-child {
    margin-top: 0;
  }

  .grid {
    display: grid;
    /* One label column for every section, so the controls line up down the
       whole dialog rather than section by section. */
    grid-template-columns: 6.2rem 1fr;
    align-items: center;
    gap: 0.55rem 0.75rem;
  }

  label,
  .label-ish {
    font-size: 0.8rem;
    color: var(--fg-dim);
  }
  /* Beside a stack of checkboxes, the label belongs with the first of them. */
  .label-ish.top {
    align-self: start;
    padding-top: 0.05rem;
  }

  /* Several controls share a row with their readout. */
  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  input[type="number"],
  input[type="text"],
  select {
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 4px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.82rem;
    padding: 0.3rem 0.4rem;
  }
  input[type="number"] {
    width: 4.5rem;
  }
  input[type="text"],
  select {
    width: 100%;
    min-width: 0;
  }
  input:focus-visible,
  select:focus-visible {
    outline: none;
    border-color: var(--accent);
  }

  .unit {
    font-size: 0.75rem;
    color: var(--fg-dim);
  }
  /* Fixed width so the row doesn't jitter as the number changes width. */
  .unit.pct {
    width: 2.6rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .checks {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.78rem;
    color: var(--fg);
    cursor: pointer;
  }
  .check input {
    accent-color: var(--accent);
    cursor: pointer;
  }
  .check.off {
    color: var(--fg-faint);
    cursor: default;
  }
  .check.off input {
    cursor: default;
  }

  .hint {
    margin: 0.4rem 0 0 6.95rem;
    font-size: 0.7rem;
    line-height: 1.35;
    color: var(--fg-dim);
  }
  code {
    font-family: "Cascadia Mono", Consolas, monospace;
    color: var(--fg);
  }

  /* Apart from the dialog's own buttons: it leaves the dialog, they act on it. */
  .config {
    margin-right: auto;
  }

  input[type="range"] {
    flex: 1;
    min-width: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
</style>
