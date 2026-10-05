<script lang="ts">
  import Dialog from "./Dialog.svelte";

  /** A yes-or-no question, asked in the app's own dialog rather than a system one. */
  export type Confirmation = {
    title: string;
    message: string;
    /** Smaller text under the message: what happens next, or how to undo it. */
    note?: string;
    /** The label on the button that goes ahead. */
    confirm: string;
    /** The action destroys something, so the button that does it is red. */
    danger?: boolean;
  };

  type Props = {
    /** The question being asked, or null when there is none. */
    question: Confirmation | null;
    onanswer: (confirmed: boolean) => void;
  };

  let { question, onanswer }: Props = $props();

  // Focus lands on Cancel: Enter on a dialog that appeared under your hands
  // should be the answer that changes nothing.
  function focusOnMount(node: HTMLElement) {
    node.focus();
  }
</script>

<!-- Escape cancels: it is the answer that changes nothing. -->
<Dialog open={question !== null} title={question?.title ?? ""} width={400} onclose={() => onanswer(false)}>
  {#if question}
    <p class="dlg-empty message">{question.message}</p>
    {#if question.note}
      <p class="dlg-note">{question.note}</p>
    {/if}

    <div class="dlg-actions">
      <button class="btn ghost" use:focusOnMount onclick={() => onanswer(false)}>Cancel</button>
      <button
        class="btn"
        class:danger={question.danger}
        class:primary={!question.danger}
        onclick={() => onanswer(true)}
      >
        {question.confirm}
      </button>
    </div>
  {/if}
</Dialog>

<style>
  .message {
    color: var(--fg);
    /* A file name can be one long unbroken word. */
    overflow-wrap: anywhere;
  }
</style>
