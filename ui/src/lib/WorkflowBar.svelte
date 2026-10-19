<script lang="ts">
  /**
   * The workflows, at the right of the bottom bar: one capsule, the one in
   * front filled, and a `+` for another. A click turns to one; a double
   * click renames it, in place; a middle click closes it.
   */
  import { tick } from "svelte";

  import type { Workflow } from "./state/workflows.svelte";

  type Props = {
    flows: Workflow[];
    current: number;
    label: (flow: Workflow) => string;
    onselect: (id: number) => void;
    onnew: () => void;
    /** An empty name gives the workflow back the name of its first folder. */
    onrename: (id: number, name: string) => void;
    oncontext: (event: MouseEvent, flow: Workflow) => void;
    onclose: (id: number) => void;
  };

  let { flows, current, label, onselect, onnew, onrename, oncontext, onclose }: Props = $props();

  /** The workflow whose name is being typed, if one is. */
  let editing = $state<number | null>(null);
  let draft = $state("");
  let field = $state<HTMLInputElement | undefined>();

  /** Open the name of a workflow for typing into. */
  export async function rename(id: number) {
    const flow = flows.find((each) => each.id === id);
    if (!flow) return;
    draft = flow.name ?? label(flow);
    editing = id;
    await tick();
    field?.focus();
    field?.select();
  }

  function finish(keep: boolean) {
    const id = editing;
    editing = null;
    if (id !== null && keep) onrename(id, draft);
  }

  function onKey(event: KeyboardEvent) {
    // The window's own keys are not for a name being typed.
    event.stopPropagation();
    if (event.key === "Enter") finish(true);
    else if (event.key === "Escape") finish(false);
  }
</script>

<span class="flows" role="group" aria-label="Workflows">
  {#each flows as flow (flow.id)}
    {#if editing === flow.id}
      <input
        bind:this={field}
        bind:value={draft}
        class="name"
        size={Math.max(4, draft.length)}
        spellcheck="false"
        aria-label="Workflow name"
        onkeydown={onKey}
        onblur={() => finish(true)}
      />
    {:else}
      <button
        class:on={flow.id === current}
        aria-pressed={flow.id === current}
        title="{label(flow)} (double-click to rename)"
        onclick={() => onselect(flow.id)}
        ondblclick={() => void rename(flow.id)}
        onauxclick={(event) => {
          if (event.button !== 1 || flows.length < 2) return;
          event.preventDefault();
          onclose(flow.id);
        }}
        oncontextmenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
          oncontext(event, flow);
        }}
      >
        {label(flow)}
      </button>
    {/if}
  {/each}
  <button class="add" title="New Workflow" aria-label="New Workflow" onclick={onnew}>+</button>
</span>

<style>
  /* As the capsule beside it, that says which face the window shows. */
  .flows {
    display: inline-flex;
    height: 17px;
    max-width: 28rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    overflow: hidden;
  }
  button,
  .name {
    padding: 0 0.65rem;
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    font: inherit;
    line-height: 17px;
  }
  button {
    max-width: 10rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  button:hover {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }
  button.on {
    background: var(--accent);
    color: var(--accent-ink);
  }
  .add {
    padding: 0 0.55rem;
  }
  .name {
    cursor: text;
    outline: none;
    background: color-mix(in srgb, var(--accent) 30%, transparent);
  }
</style>
