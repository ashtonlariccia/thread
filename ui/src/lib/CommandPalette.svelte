<script module lang="ts">
  /** Something the window can be told to do, as the palette lists it. */
  export type Command = {
    /** As it is listed, its group first: "Terminal: New Terminal". */
    title: string;
    /** The keys that do the same, if there are any. */
    keys?: string;
    /** Listed, but not to be run just now. */
    disabled?: boolean;
    run: () => void;
  };
</script>

<script lang="ts">
  /**
   * The command palette: everything the menus and the keys can do, in one
   * list to type into. Ctrl+Shift+P opens it; what is typed narrows it,
   * loosely (`nt` finds New Terminal); Enter runs the one picked out.
   */
  import { rank } from "./fuzzy";

  type Props = {
    open: boolean;
    commands: Command[];
    onclose: () => void;
  };

  let { open, commands, onclose }: Props = $props();

  let query = $state("");
  let picked = $state(0);
  let input = $state<HTMLInputElement | undefined>();
  let list = $state<HTMLElement | undefined>();

  const found = $derived(rank(query, commands, (command) => command.title));

  // Each time it opens it is empty, with the first command picked out.
  $effect(() => {
    if (!open) return;
    query = "";
    picked = 0;
    input?.focus();
  });

  // What is typed changes what is listed, and the pick goes back to the top.
  $effect(() => {
    void query;
    picked = 0;
  });

  $effect(() => {
    void picked;
    list?.querySelector(".picked")?.scrollIntoView({ block: "nearest" });
  });

  function run(command: Command | undefined) {
    if (!command || command.disabled) return;
    // Closed first: what it runs may want the focus, or open a dialog.
    onclose();
    command.run();
  }

  function onKeydown(event: KeyboardEvent) {
    const count = found.length;
    if (event.key === "Escape") onclose();
    else if (event.key === "Enter") run(found[picked]?.item);
    else if (event.key === "ArrowDown" && count > 0) picked = (picked + 1) % count;
    else if (event.key === "ArrowUp" && count > 0) picked = (picked - 1 + count) % count;
    else return;
    event.preventDefault();
    // Not the window's to hear: Escape there would do something else.
    event.stopPropagation();
  }

  /** A title in pieces, the letters that were matched set apart from the rest. */
  function pieces(title: string, at: number[]): { text: string; hit: boolean }[] {
    const hits = new Set(at);
    const out: { text: string; hit: boolean }[] = [];
    for (let i = 0; i < title.length; i++) {
      const hit = hits.has(i);
      const last = out.at(-1);
      if (last && last.hit === hit) last.text += title[i];
      else out.push({ text: title[i], hit });
    }
    return out;
  }
</script>

{#if open}
  <!-- A press anywhere else puts it away, as it does a menu. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="scrim" onmousedown={onclose}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="palette" onmousedown={(e) => e.stopPropagation()} onkeydown={onKeydown}>
      <input
        type="text"
        bind:this={input}
        bind:value={query}
        placeholder="Type a command"
        spellcheck="false"
        aria-label="Command"
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
      />
      <div class="list" id="palette-list" role="listbox" bind:this={list}>
        {#each found as { item, match }, index (item.title)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="row"
            class:picked={index === picked}
            class:disabled={item.disabled}
            role="option"
            tabindex="-1"
            aria-selected={index === picked}
            onmousemove={() => (picked = index)}
            onclick={() => run(item)}
          >
            <span class="title">
              {#each pieces(item.title, match.at) as piece, at (at)}
                {#if piece.hit}<b>{piece.text}</b>{:else}{piece.text}{/if}
              {/each}
            </span>
            {#if item.keys}
              <span class="keys">{item.keys}</span>
            {/if}
          </div>
        {:else}
          <p class="none">No command matches that.</p>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 1900;
  }

  /* Under the title bar and in the middle, where the eye already is when a
     key has just been pressed. */
  .palette {
    position: absolute;
    top: 40px;
    left: 50%;
    transform: translateX(-50%);
    width: min(560px, calc(100vw - 3rem));
    display: flex;
    flex-direction: column;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 18px 48px #000c;
    overflow: hidden;
  }

  input {
    margin: 0.5rem 0.5rem 0.35rem;
    padding: 0.4rem 0.55rem;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 5px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.86rem;
  }
  input:focus-visible {
    outline: none;
    border-color: var(--accent);
  }
  input::placeholder {
    color: var(--fg-faint);
  }

  .list {
    max-height: min(22rem, 60vh);
    overflow-y: auto;
    padding: 0 0.3rem 0.35rem;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.32rem 0.55rem;
    border-radius: var(--chip-radius);
    color: var(--fg);
    cursor: pointer;
    font-size: 0.82rem;
  }
  .row.picked {
    background: var(--hover-strong);
  }
  .row.disabled {
    color: var(--fg-faint);
    cursor: default;
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The letters that were typed, picked out in the accent. */
  .title b {
    color: var(--accent);
    font-weight: 600;
  }
  .row.disabled .title b {
    color: inherit;
  }

  .keys {
    flex: none;
    color: var(--fg-dim);
    font-size: 0.72rem;
  }

  .none {
    margin: 0.5rem 0.55rem 0.4rem;
    color: var(--fg-dim);
    font-size: 0.8rem;
  }
</style>
