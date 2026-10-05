<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import type { AppearanceStore } from "./state/appearance.svelte";

  type Props = {
    open: boolean;
    appearance: AppearanceStore;
    onclose: () => void;
  };

  let { open, appearance, onclose }: Props = $props();

  const a = $derived(appearance.current);

  /**
   * Committed on `change`, not `input`: the window resizes with the scale, and
   * resizing through 1%, 12% on the way to typing 125 would clamp to the
   * minimum and throw the window around on every keystroke.
   */
  function onScaleChange(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const value = Number(input.value);
    if (!Number.isFinite(value) || input.value === "") {
      input.value = String(a.scale);
      return;
    }
    void appearance.patch({ scale: Math.round(value) }).then(() => {
      // The store may have clamped it; the box must show what was stored even
      // when that equals the previous value and nothing re-renders.
      input.value = String(appearance.current.scale);
    });
  }
</script>

<Dialog {open} title="Appearance" width={420} {onclose}>
  <div class="grid">
    <label for="ap-opacity">Background</label>
    <div class="row">
      <!-- Floor is 20%, matching the store's clamp: a window you can't see is
           a state you can't get out of from inside it. -->
      <input
        id="ap-opacity"
        type="range"
        min="20"
        max="100"
        step="1"
        value={a.backgroundOpacity}
        oninput={(e) => void appearance.patch({ backgroundOpacity: Number(e.currentTarget.value) })}
      />
      <span class="unit pct">{a.backgroundOpacity}%</span>
    </div>

    <span class="label-ish">Material</span>
    <label class="check">
      <input
        type="checkbox"
        checked={a.material === "acrylic"}
        onchange={(e) =>
          void appearance.patch({ material: e.currentTarget.checked ? "acrylic" : "none" })}
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
        value={a.scale}
        onchange={onScaleChange}
      />
      <span class="unit">%</span>
    </div>
  </div>

  <p class="dlg-note">
    Saved as you change it. Acrylic needs the background below 100% to show through. Scale
    resizes the window along with everything in it.
  </p>

  <div class="dlg-actions">
    <button class="btn ghost" onclick={() => void appearance.reset()}>Reset</button>
    <button class="btn primary" onclick={onclose}>Done</button>
  </div>
</Dialog>

<style>
  .grid {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 0.6rem 0.75rem;
  }

  label,
  .label-ish {
    font-size: 0.8rem;
    color: var(--fg-dim);
  }

  /* Several controls share a row with their readout. */
  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  input[type="number"] {
    width: 4.5rem;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 4px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.82rem;
    padding: 0.3rem 0.4rem;
  }
  input:focus-visible {
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

  input[type="range"] {
    flex: 1;
    min-width: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
</style>
