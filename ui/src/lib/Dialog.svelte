<script module lang="ts">
  /** Per-instance id for `aria-labelledby`, so every dialog names its own title. */
  let nextId = 1;
</script>

<script lang="ts">
  import type { Snippet } from "svelte";

  type Props = {
    open: boolean;
    title: string;
    /** Panel width in px, before the viewport clamp. */
    width?: number;
    /** List bodies scroll inside a height budget; forms size to their content. */
    scrolls?: boolean;
    /** Dismiss on Escape. Pass `null` while the dialog must not be dismissed. */
    onclose: (() => void) | null;
    children: Snippet;
  };

  let { open, title, width = 520, scrolls = false, onclose, children }: Props = $props();

  const titleId = `dlg-title-${nextId++}`;

  // Guarded on `open`, so a closed dialog costs nothing per keypress.
  function onWindowKey(event: KeyboardEvent) {
    if (!open || !onclose || event.key !== "Escape") return;
    event.preventDefault();
    onclose();
  }
</script>

<svelte:window onkeydown={onWindowKey} />

{#if open}
  <div class="dlg-scrim">
    <div
      class="dlg-panel"
      class:scrolls
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      style="--dlg-width: {width}px"
    >
      <h2 class="dlg-title" id={titleId}>{title}</h2>

      {@render children()}
    </div>
  </div>
{/if}
