<script lang="ts">
  import { dropGap, dropIndex } from "./pins";

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
    /** Drop a tab at a new position. `index` counts the reordered strip. */
    onmove: (key: number, index: number) => void;
    /** Right-click on a tab. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, key: number) => void;
  };

  let { tabs, activeKey, onselect, onclose, onmove, oncontext }: Props = $props();

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
    // Every tab just moved; the drag cache no longer describes the strip.
    mids = null;
  }

  // Middle-click closes, as it does on tabs everywhere else.
  function onAuxClick(event: MouseEvent, key: number) {
    if (event.button !== 1) return;
    event.preventDefault();
    onclose(key);
  }

  // --- drag to reorder ------------------------------------------------------
  //
  // As the pins below are dragged, and for the same reasons: pointer events,
  // because the window's own drag-drop handler swallows `dragstart`, and a
  // line where the tab will land rather than tabs shuffling under the pointer.

  /** Pixels of travel before a press becomes a drag rather than a click. */
  const DRAG_SLOP = 4;

  let drag = $state<{ from: number; x: number; started: boolean; gap: number } | null>(null);

  /** Set for the duration of one click, so a finished drag doesn't also select. */
  let dragged = false;

  /** Each tab's horizontal midpoint, measured once per drag (see `PinBar`). */
  let mids: number[] | null = null;

  function midpoints(): number[] {
    if (mids) return mids;
    if (!strip) return [];
    mids = [...strip.querySelectorAll<HTMLElement>(".tab")].map((tab) => {
      const box = tab.getBoundingClientRect();
      return box.left + box.width / 2;
    });
    return mids;
  }

  function onPointerDown(event: PointerEvent, index: number) {
    // Left button only: right opens the menu, middle closes.
    if (event.button !== 0) return;
    // A press on the cross is a press on the cross. Capturing it for the tab
    // would hand the tab the click that was meant to close it.
    if ((event.target as Element).closest(".close")) return;
    dragged = false;
    mids = null;
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* no capture available for this pointer */
    }
    drag = { from: index, x: event.clientX, started: false, gap: index };
  }

  function onPointerMove(event: PointerEvent) {
    if (!drag) return;
    if (!drag.started) {
      if (Math.abs(event.clientX - drag.x) < DRAG_SLOP) return;
      drag.started = true;
    }
    drag.gap = dropGap(midpoints(), event.clientX);
  }

  function onPointerUp(event: PointerEvent) {
    const current = drag;
    drag = null;
    if (!current?.started) return;

    dragged = true;
    const index = dropIndex(midpoints(), event.clientX, current.from);
    if (index !== current.from) onmove(tabs[current.from].key, index);
  }

  function onPointerCancel() {
    drag = null;
    mids = null;
  }

  /** A drag finishes with a click on the same tab; that click is not a select. */
  function onClick(key: number) {
    if (dragged) {
      dragged = false;
      return;
    }
    onselect(key);
  }

  /**
   * Where to paint the insertion line: before the tab at this index, or past
   * the last one. Null while the gap is one the dragged tab already occupies.
   */
  const marker = $derived.by(() => {
    if (!drag?.started) return null;
    if (drag.gap === drag.from || drag.gap === drag.from + 1) return null;
    return drag.gap;
  });

  function onKeydown(event: KeyboardEvent, key: number, index: number) {
    if (event.key === "Enter" || event.key === " ") {
      onselect(key);
      return;
    }
    // Reordering without a mouse, as on the pins.
    if (event.ctrlKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault();
      const to = index + (event.key === "ArrowLeft" ? -1 : 1);
      if (to >= 0 && to < tabs.length) onmove(key, to);
    }
  }
</script>

<div
  class="tabs"
  class:dragging={drag?.started}
  role="tablist"
  bind:this={strip}
  onwheel={onWheel}
>
  {#each tabs as tab, index (tab.key)}
    <div
      class="tab"
      class:active={tab.key === activeKey}
      class:dirty={tab.dirty}
      class:lifted={drag?.started && drag.from === index}
      class:drop-before={marker === index}
      class:drop-after={marker === tabs.length && index === tabs.length - 1}
      role="tab"
      tabindex="0"
      aria-selected={tab.key === activeKey}
      title={tab.detail}
      onclick={() => onClick(tab.key)}
      onkeydown={(e) => onKeydown(e, tab.key, index)}
      onpointerdown={(e) => onPointerDown(e, index)}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerCancel}
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
    position: relative;
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

  /* While a tab is in flight the cursor says so along the whole strip. */
  .tabs.dragging,
  .tabs.dragging .tab {
    cursor: grabbing;
  }
  /* The tab being carried: dimmed, so the line reads as what is being placed. */
  .tab.lifted {
    opacity: 0.4;
  }
  /* Where it will land. */
  .drop-before::before,
  .drop-after::after {
    content: "";
    position: absolute;
    top: 3px;
    bottom: 3px;
    width: 2px;
    border-radius: 1px;
    background: var(--accent);
  }
  .drop-before::before {
    left: -1px;
  }
  .drop-after::after {
    right: 0;
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
