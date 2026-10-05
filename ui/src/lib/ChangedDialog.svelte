<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import type { Doc } from "./state/documents.svelte";

  type Props = {
    /** The file that changed on disk under unsaved edits, or null. */
    doc: Doc | null;
    onanswer: (reload: boolean) => void;
  };

  let { doc, onanswer }: Props = $props();
</script>

<!-- Escape keeps the edits: it is the answer that destroys nothing. -->
<Dialog open={doc !== null} title="File changed on disk" width={420} onclose={() => onanswer(false)}>
  {#if doc}
    <p class="dlg-empty">
      <strong>{doc.name}</strong> was changed by another program, and you have unsaved changes
      to it here.
    </p>

    <p class="dlg-note">
      Reloading discards your changes. Keeping them means your next save overwrites what is on
      disk.
    </p>

    <div class="dlg-actions">
      <button class="btn danger" onclick={() => onanswer(true)}>Reload from Disk</button>
      <button class="btn primary" onclick={() => onanswer(false)}>Keep My Changes</button>
    </div>
  {/if}
</Dialog>

<style>
  strong {
    color: var(--fg);
    font-weight: 600;
  }
</style>
