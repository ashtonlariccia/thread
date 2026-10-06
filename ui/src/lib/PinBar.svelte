<script lang="ts">
  import type { Snippet } from "svelte";

  import { dropGap, dropIndex } from "./pins";
  import type { Pin } from "./types";

  type Props = {
    pins: Pin[];
    onopen: (pin: Pin) => void;
    onunpin: (pin: Pin) => void;
    /** Drop a pin at a new position. `index` counts the reordered strip. */
    onmove: (pin: Pin, index: number) => void;
    /** Status for the right-hand end of the bar. */
    info?: Snippet;
    /** What leads the bar, from its left-hand end. */
    start?: Snippet;
  };

  let { pins, onopen, onunpin, onmove, info, start }: Props = $props();

  /**
   * Keep a group of status items to the room it has.
   *
   * As the window narrows, a group's side of the bar does too, and items that
   * no longer fit are taken out whole, one at a time, in the order their
   * `data-drop` says — lowest first. An item without one is never taken out.
   * Nothing is ever shown cut in half.
   *
   * Measured rather than done with breakpoints: how wide the items are
   * depends on what they say, and a host or a branch can be any length.
   */
  function collapsing(group: HTMLElement) {
    let queued = 0;

    const fit = () => {
      const items = [...group.children] as HTMLElement[];
      // From the top each time: an item that went may have room again.
      for (const item of items) delete item.dataset.collapsed;

      const style = getComputedStyle(group);
      const room =
        group.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
      const gap = parseFloat(style.columnGap) || 0;
      const needed = () => {
        const shown = items.filter((item) => item.dataset.collapsed === undefined);
        const widths = shown.reduce((sum, item) => sum + item.getBoundingClientRect().width, 0);
        return widths + gap * Math.max(0, shown.length - 1);
      };

      const droppable = items
        .filter((item) => item.dataset.drop !== undefined)
        .sort((a, b) => Number(a.dataset.drop) - Number(b.dataset.drop));
      for (const item of droppable) {
        if (needed() <= room) break;
        item.dataset.collapsed = "";
      }
    };

    // Once a frame at most: a drag of the window's edge reports every pixel.
    const schedule = () => {
      cancelAnimationFrame(queued);
      queued = requestAnimationFrame(fit);
    };

    // The room changes with the window; what is needed, with what the items
    // say. Attributes are not watched, so marking an item here does not ask
    // for another pass.
    const resized = new ResizeObserver(schedule);
    resized.observe(group);
    const changed = new MutationObserver(schedule);
    changed.observe(group, { childList: true, characterData: true, subtree: true });
    schedule();

    return {
      destroy() {
        cancelAnimationFrame(queued);
        resized.disconnect();
        changed.disconnect();
      },
    };
  }

  // The context menu is positioned in *viewport* coordinates rather than inside
  // the row, because the strip scrolls horizontally -- and `overflow` clips any
  // popover that tries to escape it.
  let menu = $state<{ pin: Pin; x: number; y: number } | null>(null);

  function openMenu(event: MouseEvent, pin: Pin) {
    event.preventDefault();
    menu = { pin, x: event.clientX, y: event.clientY };
  }

  function activate(pin: Pin) {
    menu = null;
    onopen(pin);
  }

  /** A drag finishes with a click on the same chip; that click is not an open. */
  function onClick(pin: Pin) {
    if (dragged) {
      dragged = false;
      return;
    }
    activate(pin);
  }

  function unpin(pin: Pin) {
    menu = null;
    onunpin(pin);
  }

  // Middle-click closes tabs everywhere else; here it removes the pin.
  function onAuxClick(event: MouseEvent, pin: Pin) {
    if (event.button !== 1) return;
    event.preventDefault();
    unpin(pin);
  }

  // --- drag to reorder ------------------------------------------------------
  //
  // Pointer events rather than HTML5 drag-and-drop: the window has the OS-level
  // drag-drop handler enabled, which swallows `dragstart` inside the webview, and
  // pointer capture gives a drag that keeps tracking even when the cursor
  // outruns the 24px-tall strip -- the same reason the sidebar resizer uses it.

  /** Pixels of travel before a press becomes a drag rather than a click. */
  const DRAG_SLOP = 4;

  let strip = $state<HTMLElement | undefined>();
  let drag = $state<{ from: number; started: boolean; gap: number } | null>(null);

  /** Set for the duration of one click, so a finished drag doesn't also open. */
  let dragged = false;

  /**
   * Horizontal midpoint of every chip, in viewport coordinates.
   *
   * Measured once per drag and reused. A pointer move fires as fast as the
   * mouse reports -- up to 1kHz -- and every `getBoundingClientRect` forces the
   * browser to flush layout, so measuring the whole strip on each one was
   * paying for a layout pass per chip per move. The strip cannot reflow mid-drag
   * (chips do not shuffle; an insertion line marks the drop instead), and the
   * one thing that does move them -- scrolling -- clears the cache.
   */
  let mids: number[] | null = null;

  function midpoints(): number[] {
    if (mids) return mids;
    if (!strip) return [];
    mids = [...strip.querySelectorAll<HTMLElement>("li")].map((li) => {
      const box = li.getBoundingClientRect();
      return box.left + box.width / 2;
    });
    return mids;
  }

  function onPointerDown(event: PointerEvent, index: number) {
    // Left button only: right opens the menu, middle unpins.
    if (event.button !== 0) return;
    // Cleared here rather than only in the click handler: pointer capture does
    // not guarantee a click follows, and a stale flag would eat the next open.
    dragged = false;
    // A fresh drag measures the strip again: chips may have been added, removed
    // or reordered since the last one.
    mids = null;
    // Capture keeps the drag alive when the cursor outruns a 24px-tall strip.
    // Not fatal if the pointer can't be captured — the drag still tracks, it
    // just stops early if the cursor leaves the chip — so don't fail the press.
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* no capture available for this pointer */
    }
    drag = { from: index, started: false, gap: index };
  }

  function onPointerMove(event: PointerEvent) {
    if (!drag) return;

    const mids = midpoints();
    if (!drag.started) {
      // A press that never really moves is a click; only commit to a drag once
      // the pointer has left the chip it started on.
      const own = mids[drag.from];
      if (own === undefined || Math.abs(event.clientX - own) < DRAG_SLOP) return;
      if (dropGap(mids, event.clientX) === drag.gap) return;
      drag.started = true;
    }

    drag.gap = dropGap(mids, event.clientX);
  }

  function onPointerUp(event: PointerEvent) {
    const current = drag;
    drag = null;
    if (!current?.started) return;

    dragged = true;
    const index = dropIndex(midpoints(), event.clientX, current.from);
    if (index !== current.from) onmove(pins[current.from], index);
  }

  function onPointerCancel() {
    drag = null;
    mids = null;
  }

  /**
   * Where to paint the insertion line: before the chip at this index, or past
   * the last one. Null while the gap is one the dragged chip already occupies,
   * so a drag that would change nothing shows nothing.
   */
  const marker = $derived.by(() => {
    if (!drag?.started) return null;
    if (drag.gap === drag.from || drag.gap === drag.from + 1) return null;
    return drag.gap;
  });

  function onKeydown(event: KeyboardEvent, pin: Pin, index: number) {
    if (event.key === "Delete") {
      event.preventDefault();
      unpin(pin);
      return;
    }

    // Reordering without a mouse. Ctrl, because bare arrows belong to moving
    // between chips.
    if (event.ctrlKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault();
      const to = index + (event.key === "ArrowLeft" ? -1 : 1);
      if (to >= 0 && to < pins.length) onmove(pin, to);
    }
  }

  function onWindowKey(event: KeyboardEvent) {
    if (event.key === "Escape") menu = null;
  }

  // A trackpad or wheel over a one-line horizontal strip produces deltaY, which
  // would otherwise do nothing at all.
  function onWheel(event: WheelEvent) {
    if (event.deltaY === 0) return;
    const strip = event.currentTarget as HTMLElement;
    if (strip.scrollWidth <= strip.clientWidth) return;
    event.preventDefault();
    strip.scrollLeft += event.deltaY;
    // Every chip just moved; the drag cache no longer describes the strip.
    mids = null;
  }
</script>

<svelte:window onkeydown={onWindowKey} onclick={() => (menu = null)} />

<footer class="pinbar">
  <!-- Two halves: one group from the left edge, the other up to the right. -->
  <div class="side">
    {#if start}
      <div class="start" use:collapsing>{@render start()}</div>
    {/if}

    <!-- Empty until something learns to pin; the bar keeps its height either
         way, so the window's frame does not change shape when the first one lands. -->
    {#if pins.length > 0}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <ul class="strip" class:dragging={drag?.started} bind:this={strip} onwheel={onWheel}>
        {#each pins as entry, index (entry.kind + ":" + entry.target)}
          <li
            class:drop-before={marker === index}
            class:drop-after={marker === pins.length && index === pins.length - 1}
          >
            <button
              class="pin"
              class:lifted={drag?.started && drag.from === index}
              title={entry.label}
              onclick={() => onClick(entry)}
              onauxclick={(e) => onAuxClick(e, entry)}
              oncontextmenu={(e) => openMenu(e, entry)}
              onkeydown={(e) => onKeydown(e, entry, index)}
              onpointerdown={(e) => onPointerDown(e, index)}
              onpointermove={onPointerMove}
              onpointerup={onPointerUp}
              onpointercancel={onPointerCancel}
            >
              <span class="label">{entry.label}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  <div class="side">
    {#if info}
      <div class="info" use:collapsing>{@render info()}</div>
    {/if}
  </div>
</footer>

{#if menu}
  {@const entry = menu.pin}
  <!-- Anchored above the cursor: the strip sits on the bottom edge, so a menu
       hanging below it would open off-screen. -->
  <div
    class="ctx"
    role="menu"
    tabindex="-1"
    style="left: {menu.x}px; bottom: {Math.max(0, window.innerHeight - menu.y + 4)}px"
  >
    <button class="ctx-item" role="menuitem" onclick={() => activate(entry)}>Open</button>
    <button class="ctx-item" role="menuitem" onclick={() => unpin(entry)}>Unpin</button>
  </div>
{/if}

<style>
  /* Deliberately thin: this is a strip of shortcuts, not a panel. It brackets
     the window against the 28px title bar without eating into the viewport.
     Chips are sized from `--pinbar-height` rather than from their own text, so
     they centre on the bar exactly. */
  .pinbar {
    --pinbar-height: 24px;

    display: grid;
    /* Half each. A group that has more to say than its half holds gives
       items up (see `collapsing`) rather than running into the other. */
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    align-items: stretch;
    height: var(--pinbar-height);
    flex: none;
    /* The mirror of the title bar. The band the eye centres on is the viewport's
       inset plus this bar, so the inset is handed to the bottom and the chips
       rise onto that band's centre line. */
    padding-bottom: var(--viewport-inset);
    user-select: none;
    overflow: hidden;
  }

  .side {
    display: flex;
    align-items: stretch;
    min-width: 0;
  }

  /* Each group starts from its own end of the bar: where the window is
     working on the left, and the file's particulars on the right. Set alike,
     so they read as one strip of status. */
  .start,
  .info {
    display: flex;
    align-items: center;
    /* Wide enough that each item is plainly its own, with no rule between. */
    gap: 1.6rem;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    /* All of it in the accent: one strip, not a colour per item. */
    color: var(--accent);
    font-size: 0.71rem;
    white-space: nowrap;
  }
  .start {
    justify-content: flex-start;
    /* The first item starts on the same vertical line as "File" above it. */
    padding: 0 0.8rem 0 var(--bar-text-inset);
  }
  .info {
    justify-content: flex-end;
    padding: 0 var(--bar-text-inset) 0 0.8rem;
  }

  /* Whole or not at all: an item keeps its width, and one there is no room
     for is taken out by `collapsing` rather than squeezed. The items belong
     to whoever supplied the snippet, hence `:global`. */
  .start > :global(*),
  .info > :global(*) {
    flex: none;
  }
  .start > :global([data-collapsed]),
  .info > :global([data-collapsed]) {
    display: none;
  }

  .strip {
    display: flex;
    align-items: center;
    gap: 1px;
    list-style: none;
    margin: 0;
    padding: 0;
    /* Between the two halves, and no wider than what is pinned. */
    flex: 0 1 auto;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
  }

  /* The default 10px scrollbar would take almost half this bar's height. */
  .strip::-webkit-scrollbar {
    height: 3px;
  }

  /* While a chip is in flight the cursor says so everywhere on the strip, not
     just over the chip it started on. */
  .strip.dragging,
  .strip.dragging .pin {
    cursor: grabbing;
  }

  li {
    position: relative;
  }

  /* Where it will land. A line in the gap rather than chips shuffling live:
     shifting the row under the pointer makes the drop target move as you chase
     it, and re-measuring a moving layout is what makes such drags jitter. */
  .drop-before::before,
  .drop-after::after {
    content: "";
    position: absolute;
    top: 2px;
    bottom: 2px;
    width: 2px;
    border-radius: 1px;
    background: var(--accent);
  }
  .drop-before::before {
    left: -1px;
  }
  .drop-after::after {
    right: -1px;
  }

  .pin {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    max-width: 200px;
    background: transparent;
    border: none;
    border-radius: var(--chip-radius);
    color: var(--fg-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.71rem;
    line-height: 1;
    /* Vertical size comes from the bar, not from the label: the chip is the bar
       less the inset top and bottom, and the label is centred in it by the
       flexbox. Padding is horizontal only. */
    height: calc(var(--pinbar-height) - 2 * var(--chip-inset));
    padding: 0 0.45rem;
    white-space: nowrap;
    transition:
      background 90ms ease,
      color 90ms ease;
  }

  .pin:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .pin:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
    color: var(--fg);
  }

  /* The chip being carried: dimmed, so the insertion line rather than the chip
     reads as the thing being positioned. After the hover rules on purpose — the
     pointer is captured on this chip, so it is hovered for the whole drag. */
  .pin.lifted,
  .pin.lifted:hover {
    opacity: 0.4;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ctx {
    position: fixed;
    z-index: 1500;
    min-width: 120px;
    padding: 0.2rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 5px;
    box-shadow: 0 8px 24px #0009;
  }

  .ctx-item {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.78rem;
    padding: 0.3rem 0.5rem;
  }
  .ctx-item:hover {
    background: var(--hover);
    color: var(--accent);
  }
</style>
