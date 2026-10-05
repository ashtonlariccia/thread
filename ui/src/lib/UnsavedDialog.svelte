<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import type { Doc, UnsavedChoice } from "./state/documents.svelte";

  type Props = {
    /** The files that would lose changes, or null when nothing is being asked. */
    docs: Doc[] | null;
    onanswer: (choice: UnsavedChoice) => void;
  };

  let { docs, onanswer }: Props = $props();
</script>

<Dialog open={docs !== null} title="Unsaved changes" width={400} onclose={() => onanswer("cancel")}>
  {#if docs}
    <p class="dlg-empty">
      {#if docs.length === 1}
        Save the changes to <strong>{docs[0].name}</strong>?
      {:else}
        Save the changes to these {docs.length} files?
      {/if}
    </p>

    {#if docs.length > 1}
      <ul class="files">
        {#each docs as doc (doc.key)}
          <li title={doc.path}>{doc.name}</li>
        {/each}
      </ul>
    {/if}

    <p class="dlg-note">They will be lost if you don't save them.</p>

    <div class="dlg-actions">
      <button class="btn ghost" onclick={() => onanswer("cancel")}>Cancel</button>
      <button class="btn danger" onclick={() => onanswer("discard")}>Don't Save</button>
      <button class="btn primary" onclick={() => onanswer("save")}>
        {docs.length === 1 ? "Save" : "Save All"}
      </button>
    </div>
  {/if}
</Dialog>

<style>
  strong {
    color: var(--fg);
    font-weight: 600;
  }

  .files {
    margin: 0.5rem 0 0;
    padding-left: 1.1rem;
    max-height: 9rem;
    overflow-y: auto;
    font-size: 0.8rem;
    color: var(--fg);
  }
</style>
