<script lang="ts">
  /**
   * The strip above the panes: which window this is, and the buttons for
   * the pane typed into. The windows themselves are in the sidebar, under
   * their sessions.
   *
   * Everything tmux does with a prefix and a key is a button here.
   */
  import type { Dir, TmuxWindow } from "./tmux.svelte";

  type Props = {
    /** The window in front. */
    window: TmuxWindow | null;
    /** What is running in the pane typed into, as tmux names it. */
    command: string;
    /**
     * Say nothing of the window here: each of its panes has a title of its
     * own, and the ones along the top are drawn where this would be.
     */
    bare: boolean;
    /** Split the pane typed into: beside it (`row`) or below it (`column`). */
    onsplit: (dir: Dir) => void;
    /** Let the pane typed into fill the window, or put it back. */
    onzoom: () => void;
    /** Close the pane typed into. */
    onclosepane: () => void;
  };

  let { window, command, bare, onsplit, onzoom, onclosepane }: Props = $props();
</script>

<div class="bar">
  <div class="title">
    {#if window && !bare}
      <span class="name">{window.name}</span>
      <!-- Said only where it says something the name does not: tmux names
           a window after what runs in it unless told otherwise. -->
      {#if command !== "" && command !== window.name}
        <span class="running">{command}</span>
      {/if}
      {#if window.paneCount > 1}
        <span class="running">{window.paneCount} panes</span>
      {/if}
    {/if}
  </div>

  <!-- For the pane the keyboard is in. Always plain to see: with no keys for
       any of it, these are the only way to do it. -->
  <div class="controls">
    <button class="ctl" title="Split pane to the right" aria-label="Split right" onclick={() => onsplit("row")}>
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
        <rect x="1" y="1.5" width="10" height="9" rx="1.6" fill="none" stroke="currentColor" />
        <path d="M6 1.5 V10.5" stroke="currentColor" />
      </svg>
    </button>
    <button class="ctl" title="Split pane downwards" aria-label="Split down" onclick={() => onsplit("column")}>
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
        <rect x="1" y="1.5" width="10" height="9" rx="1.6" fill="none" stroke="currentColor" />
        <path d="M1 6 H11" stroke="currentColor" />
      </svg>
    </button>
    {#if window && window.paneCount > 1}
      <button
        class="ctl"
        class:on={window.zoomed}
        title={window.zoomed ? "Show every pane again" : "Let this pane fill the window"}
        aria-label="Zoom pane"
        aria-pressed={window.zoomed}
        onclick={onzoom}
      >
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <path
            d="M7.2 1.5 H10.5 V4.8 M4.8 10.5 H1.5 V7.2 M10.5 1.5 L7 5 M1.5 10.5 L5 7"
            fill="none"
            stroke="currentColor"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
      <button class="ctl" title="Close this pane" aria-label="Close pane" onclick={onclosepane}>
        <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
          <path
            d="M1.5 1.5 L8.5 8.5 M8.5 1.5 L1.5 8.5"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
          />
        </svg>
      </button>
    {/if}
  </div>
</div>

<style>
  /* As tall as the editor's strip of tabs, the two being the same strip in
     the same place. */
  .bar {
    flex: none;
    display: flex;
    align-items: stretch;
    height: var(--tabs-height);
    user-select: none;
  }

  .title {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 0.7rem;
    padding: 0 0.75rem;
    overflow: hidden;
    font-size: 0.74rem;
    white-space: nowrap;
  }
  .name {
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .running {
    flex: none;
    color: var(--fg-dim);
  }

  .controls {
    flex: none;
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 0 4px;
  }
  .ctl {
    display: grid;
    place-items: center;
    width: 20px;
    height: 18px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .ctl:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .ctl.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
</style>
