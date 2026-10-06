<script lang="ts">
  import type { SplitDir } from "./panes";
  import { dropGap } from "./pins";
  import { tabDrag, type DropTarget } from "./state/tabDrag.svelte";

  /** One open file or terminal, as the strip shows it. */
  export type Tab = {
    key: number;
    name: string;
    /** For the tooltip: the full path, or a note that there is none yet. */
    detail: string;
    icon: string;
    dirty: boolean;
  };

  type Props = {
    /** The pane this is the strip of. */
    pane: number;
    /** Whether that pane is the one the keyboard is in. */
    focused: boolean;
    tabs: Tab[];
    activeKey: number | null;
    /** Whether there is another pane, and so a point in closing this one. */
    closable: boolean;
    onselect: (key: number) => void;
    onclose: (key: number) => void;
    /**
     * Drop a tab at a new position, in this pane or another. `index` counts
     * the strip it lands in as it will be without the tab; negative is its end.
     */
    onmove: (key: number, to: number, index: number) => void;
    /** Right-click on a tab. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, key: number) => void;
    /** Another pane beside this one (`row`) or below it (`column`). */
    onsplit: (dir: SplitDir) => void;
    onclosepane: () => void;
  };

  let {
    pane,
    focused,
    tabs,
    activeKey,
    closable,
    onselect,
    onclose,
    onmove,
    oncontext,
    onsplit,
    onclosepane,
  }: Props = $props();

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
    mids.clear();
  }

  // Middle-click closes, as it does on tabs everywhere else.
  function onAuxClick(event: MouseEvent, key: number) {
    if (event.button !== 1) return;
    event.preventDefault();
    onclose(key);
  }

  // --- drag to reorder, and to another pane -----------------------------------
  //
  // As the pins below are dragged, and for the same reasons: pointer events,
  // because the window's own drag-drop handler swallows `dragstart`, and a
  // line where the tab will land rather than tabs shuffling under the pointer.
  //
  // The strip a tab is dropped on need not be the one it left. Every strip
  // marks itself with its pane, so whichever is under the pointer can be
  // found and measured from here, and the line is drawn by the strip it is
  // over (see `tabDrag`). Dropped on the body of a pane, a tab joins the end
  // of that pane's strip.

  /** Pixels of travel before a press becomes a drag rather than a click. */
  const DRAG_SLOP = 4;

  let drag = $state<{ from: number; x: number; started: boolean } | null>(null);

  /** Set for the duration of one click, so a finished drag doesn't also select. */
  let dragged = false;

  /** Each strip's tabs' horizontal midpoints, measured once per drag (see `PinBar`). */
  const mids = new Map<Element, number[]>();

  function midpoints(of: Element): number[] {
    let found = mids.get(of);
    if (!found) {
      found = [...of.querySelectorAll<HTMLElement>(".tab")].map((tab) => {
        const box = tab.getBoundingClientRect();
        return box.left + box.width / 2;
      });
      mids.set(of, found);
    }
    return found;
  }

  /** What is under the pointer that a tab can be dropped on. */
  function targetAt(x: number, y: number): DropTarget | null {
    const under = document.elementFromPoint(x, y);
    const over = under?.closest<HTMLElement>("[data-tabstrip]");
    if (over) return { pane: Number(over.dataset.tabstrip), gap: dropGap(midpoints(over), x) };
    const body = under?.closest<HTMLElement>("[data-pane]");
    return body ? { pane: Number(body.dataset.pane), gap: null } : null;
  }

  function onPointerDown(event: PointerEvent, index: number) {
    // Left button only: right opens the menu, middle closes.
    if (event.button !== 0) return;
    // A press on the cross is a press on the cross. Capturing it for the tab
    // would hand the tab the click that was meant to close it.
    if ((event.target as Element).closest(".close")) return;
    dragged = false;
    mids.clear();
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* no capture available for this pointer */
    }
    drag = { from: index, x: event.clientX, started: false };
  }

  function onPointerMove(event: PointerEvent) {
    if (!drag) return;
    if (!drag.started) {
      if (Math.abs(event.clientX - drag.x) < DRAG_SLOP) return;
      drag.started = true;
      tabDrag.from = { pane, index: drag.from };
    }
    tabDrag.over = targetAt(event.clientX, event.clientY);
  }

  function endDrag() {
    drag = null;
    mids.clear();
    tabDrag.from = null;
    tabDrag.over = null;
  }

  function onPointerUp() {
    const current = drag;
    const over = tabDrag.over;
    endDrag();
    if (!current?.started) return;

    dragged = true;
    if (!over) return;
    const key = tabs[current.from].key;
    if (over.pane !== pane) {
      onmove(key, over.pane, over.gap ?? -1);
      return;
    }
    // Its own strip. A gap to the right of where it started counts one too
    // many, the count having included the tab itself (see `dropIndex`).
    if (over.gap === null) return;
    const index = over.gap > current.from ? over.gap - 1 : over.gap;
    if (index !== current.from) onmove(key, pane, index);
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
    const { from, over } = tabDrag;
    if (!from || over?.pane !== pane || over.gap === null) return null;
    if (from.pane === pane && (over.gap === from.index || over.gap === from.index + 1)) return null;
    return over.gap;
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
      if (to >= 0 && to < tabs.length) onmove(key, pane, to);
    }
  }
</script>

<div class="bar" class:focused data-tabstrip={pane}>
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
        onpointercancel={endDrag}
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

  <!-- The pane's own controls, at the end of its strip: what `:vsp`, `:sp`
       and `:close` do, for the pointer. -->
  <div class="controls">
    <button class="ctl" title="Split right (:vsp)" aria-label="Split right" onclick={() => onsplit("row")}>
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
        <rect x="1" y="1.5" width="10" height="9" rx="1.6" fill="none" stroke="currentColor" />
        <path d="M6 1.5 V10.5" stroke="currentColor" />
      </svg>
    </button>
    <button class="ctl" title="Split down (:sp)" aria-label="Split down" onclick={() => onsplit("column")}>
      <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
        <rect x="1" y="1.5" width="10" height="9" rx="1.6" fill="none" stroke="currentColor" />
        <path d="M1 6 H11" stroke="currentColor" />
      </svg>
    </button>
    {#if closable}
      <button class="ctl" title="Close pane (:close)" aria-label="Close pane" onclick={onclosepane}>
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
  /* Deliberately thin: it names the files, it is not a toolbar. Sits inside
     the viewport card, so it paints nothing of its own and shares the card's
     surface with the editor below, with no line between them. */
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
  /* The tab showing in a pane the keyboard is not in is marked, and quietly:
     the accent is for the one that keys will go to. */
  .tab.active {
    background: var(--hover);
    border-bottom-color: var(--fg-faint);
    color: var(--fg);
  }
  .bar.focused .tab.active {
    background: var(--accent-soft);
    border-bottom-color: var(--accent);
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

  /* Faint until the pointer is on the strip: three buttons on every pane is
     a lot of buttons for something done a few times a day. */
  .controls {
    flex: none;
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 0 4px;
    opacity: 0.35;
    transition: opacity 90ms ease;
  }
  .bar:hover .controls,
  .controls:focus-within {
    opacity: 1;
  }
  .ctl {
    width: 20px;
    height: 18px;
  }
  .ctl:hover {
    background: var(--hover);
    color: var(--fg);
  }
</style>
