<script lang="ts">
  /**
   * The strip above a session: a tab for each of its tmux windows, and at
   * its end the buttons for the window in front and the pane typed into.
   *
   * Everything tmux does with a prefix and a key is a button here.
   */
  import { iconUrl } from "../icons";
  import type { Dir, TmuxWindow } from "./tmux.svelte";

  type Props = {
    windows: TmuxWindow[];
    activeId: number | null;
    onselect: (window: number) => void;
    /** Close a window, and with it every pane in it. */
    onclose: (window: number) => void;
    onnew: () => void;
    onrename: (window: number, name: string) => void;
    /** Right-click on a tab. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, window: number) => void;
    /** Split the pane typed into: beside it (`row`) or below it (`column`). */
    onsplit: (dir: Dir) => void;
    /** Let the pane typed into fill the window, or put it back. */
    onzoom: () => void;
    /** Close the pane typed into. */
    onclosepane: () => void;
  };

  let {
    windows,
    activeId,
    onselect,
    onclose,
    onnew,
    onrename,
    oncontext,
    onsplit,
    onzoom,
    onclosepane,
  }: Props = $props();

  const active = $derived(windows.find((each) => each.id === activeId) ?? null);

  let strip = $state<HTMLElement | undefined>();

  // Keep the tab in front on screen: a new window lands at the far end.
  $effect(() => {
    if (activeId === null) return;
    strip?.querySelector(".tab.active")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });

  function onWheel(event: WheelEvent) {
    if (event.deltaY === 0 || !strip || strip.scrollWidth <= strip.clientWidth) return;
    event.preventDefault();
    strip.scrollLeft += event.deltaY;
  }

  // --- renaming -------------------------------------------------------------------

  /** The window whose name is being typed over. */
  let renaming = $state<number | null>(null);
  /** Enter settles the box and removes it, and removing it blurs it: one answer. */
  let settled = false;

  /** Type a new name onto a window's tab. */
  export function rename(window: number) {
    renaming = window;
  }

  function settle(window: TmuxWindow, value: string | null) {
    if (settled) return;
    settled = true;
    renaming = null;
    const name = value?.trim() ?? "";
    if (name !== "" && name !== window.name) onrename(window.id, name);
  }

  function nameBox(node: HTMLInputElement) {
    settled = false;
    node.focus();
    node.select();
  }

  function onNameKey(event: KeyboardEvent, window: TmuxWindow) {
    event.stopPropagation();
    if (event.key === "Enter") settle(window, (event.currentTarget as HTMLInputElement).value);
    else if (event.key === "Escape") settle(window, null);
  }
</script>

<div class="bar">
  <div class="tabs" role="tablist" bind:this={strip} onwheel={onWheel}>
    {#each windows as window (window.id)}
      <div
        class="tab"
        class:active={window.id === activeId}
        role="tab"
        tabindex="0"
        aria-selected={window.id === activeId}
        title={renaming === window.id ? undefined : `${window.index}: ${window.name}`}
        onclick={() => onselect(window.id)}
        ondblclick={() => rename(window.id)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") onselect(window.id);
        }}
        onauxclick={(e) => {
          // Middle-click closes, as it does on tabs everywhere else.
          if (e.button !== 1) return;
          e.preventDefault();
          onclose(window.id);
        }}
        onmousedown={(e) => {
          // Stops the middle button starting the webview's autoscroll.
          if (e.button === 1) e.preventDefault();
        }}
        oncontextmenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          oncontext(e, window.id);
        }}
      >
        <img src={iconUrl("console")} alt="" width="14" height="14" draggable="false" />
        {#if renaming === window.id}
          <input
            class="rename"
            value={window.name}
            spellcheck="false"
            autocomplete="off"
            aria-label="Window name"
            use:nameBox
            onkeydown={(e) => onNameKey(e, window)}
            onclick={(e) => e.stopPropagation()}
            ondblclick={(e) => e.stopPropagation()}
            onblur={(e) => settle(window, e.currentTarget.value)}
          />
        {:else}
          <span class="name">{window.name}</span>
        {/if}
        <button
          class="close"
          aria-label="Close {window.name}"
          tabindex="-1"
          onclick={(e) => {
            e.stopPropagation();
            onclose(window.id);
          }}
        >
          <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
            <path
              d="M1.5 1.5 L8.5 8.5 M8.5 1.5 L1.5 8.5"
              stroke="currentColor"
              stroke-width="1.3"
              stroke-linecap="round"
            />
          </svg>
        </button>
      </div>
    {/each}

    <button class="ctl new" title="New window" aria-label="New window" onclick={onnew}>
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M5 1 V9 M1 5 H9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
      </svg>
    </button>
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
    {#if active && active.paneCount > 1}
      <button
        class="ctl"
        class:on={active.zoomed}
        title={active.zoomed ? "Show every pane again" : "Let this pane fill the window"}
        aria-label="Zoom pane"
        aria-pressed={active.zoomed}
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
  /* Set as the editor's strip is (`TabBar`), the two being the same strip
     in the same place. */
  .bar {
    flex: none;
    display: flex;
    align-items: stretch;
    height: var(--tabs-height);
    user-select: none;
  }

  .tabs {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: stretch;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .tabs::-webkit-scrollbar {
    display: none;
  }

  .tab {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    max-width: 220px;
    padding: 0 0.3rem 0 0.6rem;
    border-bottom: 1px solid transparent;
    color: var(--fg-dim);
    cursor: pointer;
    font-size: 0.74rem;
    transition:
      background 90ms ease,
      color 90ms ease;
  }
  .tab:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .tab.active {
    background: var(--accent-soft);
    border-bottom-color: var(--accent);
    color: var(--fg);
  }
  .tab:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }

  .tab img {
    flex: none;
    display: block;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rename {
    width: 9rem;
    min-width: 0;
    height: 18px;
    padding: 0 0.25rem;
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: 3px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.74rem;
    outline: none;
  }

  .close,
  .ctl {
    flex: none;
    display: grid;
    place-items: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .close {
    width: 16px;
    height: 16px;
  }
  .close:hover {
    background: #f38ba81f;
    color: var(--danger);
  }
  .close svg {
    visibility: hidden;
  }
  .tab:hover .close svg,
  .tab.active .close svg {
    visibility: visible;
  }

  .controls {
    flex: none;
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 0 4px;
  }
  .ctl {
    width: 20px;
    height: 18px;
  }
  .ctl.new {
    align-self: center;
    margin-left: 3px;
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
