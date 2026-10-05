<script lang="ts">
  import type { SidebarItem } from "./types";

  type Props = {
    /** Which edge of the window it sits on; everything that points mirrors. */
    side: "left" | "right";
    items: SidebarItem[];
    activeKey: number | null;
    width: number;
    collapsed: boolean;
    /** A drag is in flight, so the width must track the pointer, not glide. */
    resizing: boolean;
    onselect: (key: number) => void;
    onclose: (key: number) => void;
    /** Right-click on a row. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, item: SidebarItem) => void;
  };

  let {
    side,
    items,
    activeKey,
    width,
    collapsed,
    resizing,
    onselect,
    onclose,
    oncontext,
  }: Props = $props();

  // --- hover card ------------------------------------------------------------
  //
  // Not a `title` attribute. The native tooltip is a yellow-white system chip
  // that arrives after a second or so, ignores the theme, and — collapsed —
  // is the *only* thing naming the row, which is too important a job for a
  // control the app cannot style.
  //
  // Rendered fixed and outside the `<aside>`, because the list scrolls, and
  // `overflow` on the list clips anything that tries to sit beside a row.

  /** Long enough that sweeping down the list doesn't flash a card per row. */
  const HOVER_DELAY_MS = 260;

  let aside = $state<HTMLElement | undefined>();
  let tip = $state<{ item: SidebarItem; x: number; y: number } | null>(null);
  let tipTimer: ReturnType<typeof setTimeout> | undefined;

  function showTipSoon(event: MouseEvent, item: SidebarItem) {
    const row = event.currentTarget as HTMLElement;
    clearTimeout(tipTimer);
    tipTimer = setTimeout(() => {
      const box = row.getBoundingClientRect();
      const rail = aside?.getBoundingClientRect();
      tip = {
        item,
        // Beside the sidebar rather than beside the row: rows are inset, and a
        // card that tracked them would step in and out as the list scrolled.
        // On the stage side of it, whichever side that is.
        x: side === "left" ? (rail?.right ?? box.right) + 6 : (rail?.left ?? box.left) - 6,
        y: box.top + box.height / 2,
      };
    }, HOVER_DELAY_MS);
  }

  function hideTip() {
    clearTimeout(tipTimer);
    tip = null;
  }

  // Don't leave a card scheduled for a sidebar that is being torn down.
  $effect(() => () => clearTimeout(tipTimer));
</script>

<aside
  style="width: {width}px"
  class:collapsed
  class:resizing
  class:right={side === "right"}
  bind:this={aside}
>
  <!-- Scrolling moves every row out from under its card. -->
  <ul onscroll={hideTip}>
    {#each items as item (item.key)}
      <li>
        <!-- Collapsed, the name is off the screen, so the hover card has to
             carry it -- otherwise the rail is a column of anonymous icons. -->
        <div
          class="row"
          class:active={item.key === activeKey}
          role="button"
          tabindex="0"
          onclick={() => onselect(item.key)}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onselect(item.key)}
          oncontextmenu={(e) => {
            // Stopped, not merely defaulted: the window-level handler in App
            // opens the plain menu, and this row wants the one with Close on it.
            e.preventDefault();
            e.stopPropagation();
            hideTip();
            oncontext(e, item);
          }}
          onmouseenter={(e) => showTipSoon(e, item)}
          onmouseleave={hideTip}
          onpointerdown={hideTip}
        >
          <span class="glyph">
            {#if item.icon}
              <img src={item.icon} alt="" width="16" height="16" draggable="false" />
            {:else}
              <span class="dot"></span>
            {/if}
            <!-- Collapsed, there is no label to carry the marker, so it rides
                 on the icon's corner instead. -->
            {#if item.dirty && collapsed}
              <span class="dirty badge"></span>
            {/if}
          </span>
          {#if !collapsed}
            <span class="label">{item.title}</span>
            {#if item.dirty}
              <span class="dirty" aria-label="Unsaved changes"></span>
            {/if}
            <!-- aria-label, not `title`: a native tooltip here would fight the
                 hover card the row is already showing. -->
            <button
              class="kill"
              aria-label="Close"
              onclick={(e) => {
                e.stopPropagation();
                onclose(item.key);
              }}
            >
              ×
            </button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>
</aside>

{#if tip}
  <!-- Centred on the row it describes, clamped so a row near the bottom of a
       full list still gets a card that is entirely on screen. -->
  <div
    class="tip"
    class:right={side === "right"}
    role="tooltip"
    style="left: {tip.x}px; top: {Math.min(Math.max(tip.y, 20), window.innerHeight - 20)}px"
  >
    <span class="tip-name">{tip.item.title}</span>
    {#if tip.item.detail}
      <span class="tip-detail">{tip.item.detail}</span>
    {/if}
  </div>
{/if}

<style>
  aside {
    /* Width is driven by the drag handle in App.svelte, or pinned to the rail
       width while collapsed. */
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    transition: width 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
    /* NOTE: no overflow here. `overflow` creates a clipping context, which
       would cut off anything that extends past the sidebar. Scrolling belongs
       on the list itself. */
  }

  /* Dragging the handle sets the width every pointer move; easing each one
     would make the sidebar lag behind the cursor. */
  aside.resizing {
    transition: none;
  }

  @media (prefers-reduced-motion: reduce) {
    aside {
      transition: none;
    }
  }

  ul {
    list-style: none;
    margin: 0;
    /* Inset so hover chips never touch the sidebar's edges. */
    padding: 0.3rem 0.35rem;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  li + li {
    margin-top: 1px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.35rem 0.45rem;
    border-radius: var(--chip-radius);
    cursor: pointer;
    user-select: none;
  }

  .row:hover {
    background: var(--hover);
  }

  .row.active {
    background: var(--accent-soft);
  }

  .glyph {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
  }
  .glyph img {
    display: block;
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }

  /* Unsaved changes: the same mark VS Code puts on a tab. */
  .dirty {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--fg-dim);
  }
  .dirty.badge {
    position: absolute;
    top: -3px;
    right: -4px;
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.82rem;
    color: var(--fg);
  }

  .kill {
    background: transparent;
    border: none;
    color: var(--fg-dim);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    padding: 0 0.15rem;
    border-radius: 3px;
  }
  .kill:hover {
    color: var(--danger);
    background: #f38ba81f;
  }

  /* --- collapsed rail ----------------------------------------------------- */

  .collapsed ul {
    padding: 0.3rem 0;
    /* Centred in what the eye reads as the rail: the band between the window
       edge and the viewport's border, which is the rail *plus* the viewport's
       inset. Centring in the rail alone leaves the icons visibly nearer the
       window edge, so they are nudged half the inset towards the viewport. */
    position: relative;
    left: calc(var(--viewport-inset) / 2);
  }
  .right.collapsed ul {
    left: calc(var(--viewport-inset) / -2);
  }

  /* The icon is the whole row, so centre it and drop the text-sized padding. */
  .collapsed .row {
    justify-content: center;
    padding: 0.4rem 0;
    margin: 0 0.25rem;
  }

  /* --- hover card --------------------------------------------------------- */

  /* Same surface as the menus, and opaque for the same reason they are: a card
     you can read the viewport through is not a card. It sits below the context
     menu's layer so the two never stack. */
  .tip {
    position: fixed;
    z-index: 1400;
    transform: translateY(-50%);
    display: flex;
    flex-direction: column;
    gap: 1px;
    max-width: 420px;
    padding: 0.35rem 0.55rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: var(--chip-radius);
    box-shadow: 0 6px 18px #0009;
    pointer-events: none;
    /* Appears rather than pops. The delay already did the waiting; this is
       just so it doesn't snap into existence at full contrast. */
    animation: tip-in 110ms ease-out;
  }

  /* `left` is the sidebar's inner edge either way; on the right the card
     hangs back from it instead of forward. */
  .tip.right {
    transform: translate(-100%, -50%);
    animation-name: tip-in-right;
  }

  @keyframes tip-in {
    from {
      opacity: 0;
      transform: translateY(-50%) translateX(-3px);
    }
  }
  @keyframes tip-in-right {
    from {
      opacity: 0;
      transform: translate(calc(-100% + 3px), -50%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .tip {
      animation: none;
    }
  }

  .tip-name {
    font-size: 0.8rem;
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tip-detail {
    font-size: 0.68rem;
    color: var(--fg-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
