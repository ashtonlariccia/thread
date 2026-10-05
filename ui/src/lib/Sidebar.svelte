<script lang="ts">
  import type { SidebarItem } from "./types";

  type Props = {
    items: SidebarItem[];
    activeKey: string | null;
    width: number;
    collapsed: boolean;
    /** A drag is in flight, so the width must track the pointer, not glide. */
    resizing: boolean;
    /** What to say when there is nothing to list. */
    empty?: string;
    onselect: (item: SidebarItem) => void;
    /** Right-click on a row. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, item: SidebarItem) => void;
  };

  let { items, activeKey, width, collapsed, resizing, empty, onselect, oncontext }: Props =
    $props();

  // Keep the active row on screen: in a long tree the file just opened may be
  // well below the fold. Keyed on the active row alone, so unfolding folders
  // or the list changing under it never yanks the scroll position.
  let list = $state<HTMLElement | undefined>();
  $effect(() => {
    if (activeKey === null) return;
    list?.querySelector(".row.active")?.scrollIntoView({ block: "nearest" });
  });
</script>

<aside style="width: {width}px" class:collapsed class:resizing>
  <!-- Collapsed, the sidebar is an empty rail: the tree fades out rather than
       shrinking into a column of icons. `inert` takes it out of the tab order
       and away from the pointer while it is not there to be seen. -->
  <div class="content" inert={collapsed}>
    {#if items.length === 0 && empty}
      <p class="empty">{empty}</p>
    {/if}

    <ul bind:this={list}>
      {#each items as item (item.key)}
        <li>
          <div
            class="row"
            class:active={item.key === activeKey}
            role="button"
            tabindex="0"
            title={item.path}
            aria-expanded={item.folder ? item.folder === "open" : undefined}
            onclick={() => onselect(item)}
            onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onselect(item)}
            oncontextmenu={(e) => {
              // Stopped, not merely defaulted: the window-level handler in App
              // opens the plain menu, and a row may want one of its own.
              e.preventDefault();
              e.stopPropagation();
              oncontext(e, item);
            }}
          >
            <!-- One per level of nesting. Each draws a hairline where its
                 ancestor's chevron sits, so the lines of consecutive rows
                 join into one running down from the folder they belong to. -->
            {#each { length: item.depth } as _, level (level)}
              <span class="guide"></span>
            {/each}
            <!-- Files keep the chevron's space, so their icons line up with
                 the folders' rather than stepping left. -->
            <span class="chevron" class:open={item.folder === "open"}>
              {#if item.folder}
                <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
                  <path
                    d="M3.5 2 L6.5 5 L3.5 8"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="1.3"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                  />
                </svg>
              {/if}
            </span>
            <img class="glyph" src={item.icon} alt="" width="16" height="16" draggable="false" />
            <span class="label">{item.title}</span>
            {#if item.dirty}
              <span class="dirty" aria-label="Unsaved changes"></span>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  </div>
</aside>

<style>
  aside {
    /* Width is driven by the drag handle in App.svelte, or pinned to the rail
       width while collapsed. */
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    transition: width 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
  }

  /* Dragging the handle sets the width every pointer move; easing each one
     would make the sidebar lag behind the cursor. */
  aside.resizing {
    transition: none;
  }

  .content {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    /* The rows are wider than the sidebar while it is closing; without this
       they would spill over the viewport on their way out. */
    overflow: hidden;
    transition: opacity 140ms ease;
  }

  /* Gone a little before the sidebar finishes closing, so what is left to
     watch is the edge moving rather than text being squeezed. */
  .collapsed .content {
    opacity: 0;
    transition-duration: 90ms;
  }

  @media (prefers-reduced-motion: reduce) {
    aside,
    .content {
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
    overflow-x: hidden;
    overflow-y: auto;
  }

  /* Spacing comes from the pieces rather than a flex gap: a gap would open up
     between the guides, and they have to sit on an exact grid. Stretched
     rather than centred so each guide is the full height of its row -- and
     rows touch, so the guide lines of one run straight into the next. */
  .row {
    display: flex;
    align-items: stretch;
    min-height: 22px;
    padding: 0 0.35rem;
    border-radius: var(--chip-radius);
    cursor: pointer;
    user-select: none;
  }
  .row > * {
    align-self: center;
  }

  .row:hover {
    background: var(--hover);
  }

  .row.active {
    background: var(--accent-soft);
  }

  /* One level of indent. The hairline sits where the centre of the parent's
     chevron is -- half of the 14px the chevron takes up -- so it reads as
     hanging from that folder. Faint: it is there to be followed by the eye
     when needed, not looked at. */
  .row > .guide {
    align-self: stretch;
    flex: none;
    width: 14px;
    background: linear-gradient(#ffffff14, #ffffff14) 6.5px 0 / 1px 100% no-repeat;
  }

  .chevron {
    flex: none;
    display: grid;
    place-items: center;
    width: 14px;
    height: 14px;
    color: var(--fg-dim);
  }
  .chevron svg {
    transition: transform 90ms ease;
  }
  .chevron.open svg {
    transform: rotate(90deg);
  }

  .glyph {
    flex: none;
    display: block;
    margin: 0 0.4rem 0 0.15rem;
  }

  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.82rem;
    color: var(--fg);
  }

  /* Unsaved changes: the same dot the file's tab wears, at the row's far end
     so a column of them can be scanned down the sidebar's edge. */
  .dirty {
    flex: none;
    width: 7px;
    height: 7px;
    margin: 0 0.2rem 0 0.4rem;
    border-radius: 50%;
    background: var(--fg-dim);
  }

  .empty {
    margin: 0;
    padding: 0.6rem 0.8rem 0;
    color: var(--fg-faint);
    font-size: 0.75rem;
    line-height: 1.4;
    user-select: none;
  }
</style>
