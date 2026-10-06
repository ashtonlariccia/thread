<script lang="ts">
  /** One open file, as the strip shows it. */
  export type Tab = {
    key: number;
    name: string;
    /** For the tooltip: the full path, or a note that there is none yet. */
    detail: string;
    icon: string;
    dirty: boolean;
  };

  type Props = {
    tabs: Tab[];
    activeKey: number | null;
    onselect: (key: number) => void;
    onclose: (key: number) => void;
    /** Right-click on a tab. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, key: number) => void;
  };

  let { tabs, activeKey, onselect, onclose, oncontext }: Props = $props();

  let strip = $state<HTMLElement | undefined>();

  // Keep the active tab on screen: Ctrl+Tab, or a file opened from the tree,
  // can land on one that is scrolled out of the strip.
  $effect(() => {
    if (activeKey === null) return;
    strip?.querySelector(".tab.active")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  });

  // A wheel over a one-line horizontal strip produces deltaY, which would
  // otherwise do nothing at all.
  function onWheel(event: WheelEvent) {
    if (event.deltaY === 0 || !strip || strip.scrollWidth <= strip.clientWidth) return;
    event.preventDefault();
    strip.scrollLeft += event.deltaY;
  }

  // Middle-click closes, as it does on tabs everywhere else.
  function onAuxClick(event: MouseEvent, key: number) {
    if (event.button !== 1) return;
    event.preventDefault();
    onclose(key);
  }
</script>

<div class="tabs" role="tablist" bind:this={strip} onwheel={onWheel}>
  {#each tabs as tab (tab.key)}
    <div
      class="tab"
      class:active={tab.key === activeKey}
      class:dirty={tab.dirty}
      role="tab"
      tabindex="0"
      aria-selected={tab.key === activeKey}
      title={tab.detail}
      onclick={() => onselect(tab.key)}
      onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onselect(tab.key)}
      onauxclick={(e) => onAuxClick(e, tab.key)}
      onmousedown={(e) => {
        // Stops the middle button starting the webview's autoscroll.
        if (e.button === 1) e.preventDefault();
      }}
      oncontextmenu={(e) => {
        // Stopped, not merely defaulted: the window-level handler in App
        // opens the plain menu, and a tab wants the one with Close on it.
        e.preventDefault();
        e.stopPropagation();
        oncontext(e, tab.key);
      }}
    >
      <img src={tab.icon} alt="" width="14" height="14" draggable="false" />
      <span class="name">{tab.name}</span>
      <!-- One slot for both marks: the dot says "unsaved" until the pointer
           arrives, when it gives way to the button that closes the tab. -->
      <button
        class="close"
        aria-label="Close {tab.name}"
        tabindex="-1"
        onclick={(e) => {
          e.stopPropagation();
          onclose(tab.key);
        }}
      >
        <span class="dot"></span>
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
</div>

<style>
  /* Deliberately thin: it names the files, it is not a toolbar. Sits inside
     the viewport card, so it paints nothing of its own and shares the card's
     surface with the editor below, with no line between them. */
  .tabs {
    --tabs-height: 26px;

    flex: none;
    display: flex;
    align-items: stretch;
    height: var(--tabs-height);
    overflow-x: auto;
    overflow-y: hidden;
    user-select: none;
  }

  /* A scrollbar of any height would be a third of this strip. The wheel and
     the active tab scrolling itself into view do the job instead. */
  .tabs::-webkit-scrollbar {
    display: none;
  }

  .tab {
    flex: none;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    max-width: 220px;
    padding: 0 0.3rem 0 0.6rem;
    /* Only the active tab's shows: the underline that says which one it is. */
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

  .close {
    flex: none;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .close:hover {
    background: #f38ba81f;
    color: var(--danger);
  }

  /* The cross is there on the tab you are on or pointing at; elsewhere the
     slot stays empty so a row of tabs is not a row of buttons. */
  .close svg {
    visibility: hidden;
  }
  .tab:hover .close svg,
  .tab.active .close svg {
    visibility: visible;
  }

  .dot {
    display: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--fg-dim);
  }
  /* Unsaved: the dot takes the slot, until the pointer is on the tab. */
  .tab.dirty:not(:hover) .dot {
    display: block;
  }
  .tab.dirty:not(:hover) .close svg {
    display: none;
  }
  .tab.dirty:hover .close svg {
    visibility: visible;
  }
</style>
