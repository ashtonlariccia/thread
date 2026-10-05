<script lang="ts">
  import type { SidebarItem } from "./types";

  type Props = {
    items: SidebarItem[];
    /** The row for the file being edited. */
    activeKey: string | null;
    /** The row last clicked: what New File, Rename and Delete act on. */
    selectedKey: string | null;
    width: number;
    collapsed: boolean;
    /** A drag is in flight, so the width must track the pointer, not glide. */
    resizing: boolean;
    /** What to say when there is nothing to list. */
    empty?: string;
    onselect: (item: SidebarItem) => void;
    /** A key pressed on a focused row, for the row's own shortcuts. */
    onkey: (event: KeyboardEvent, item: SidebarItem) => void;
    /** The name box was settled: with what was typed, or null if abandoned. */
    onedit: (value: string | null) => void;
    /**
     * Right-click on a row, or — with no item — on the empty space below the
     * rows. App owns the menu, so only one is ever open.
     */
    oncontext: (event: MouseEvent, item: SidebarItem | null) => void;
  };

  let {
    items,
    activeKey,
    selectedKey,
    width,
    collapsed,
    resizing,
    empty,
    onselect,
    onkey,
    onedit,
    oncontext,
  }: Props = $props();

  // Keep the active row on screen: in a long tree the file just opened may be
  // well below the fold. Keyed on the active row alone, so unfolding folders
  // or the list changing under it never yanks the scroll position.
  let list = $state<HTMLElement | undefined>();
  $effect(() => {
    if (activeKey === null) return;
    list?.querySelector(".row.active")?.scrollIntoView({ block: "nearest" });
  });

  // --- the name box ------------------------------------------------------------
  //
  // New File, New Folder and Rename all type a name straight into the tree,
  // on the row where the entry is or will be.

  /**
   * Enter settles the box and removes it, and removing a focused input fires
   * `blur`, which settles it too. One box, one answer.
   */
  let settled = false;

  function settle(value: string | null) {
    if (settled) return;
    settled = true;
    onedit(value);
  }

  function nameBox(node: HTMLInputElement) {
    settled = false;
    node.focus();
    // Renaming `main.rs` is nearly always about `main`; leave the extension
    // out of the selection so typing replaces only the stem.
    const dot = node.value.lastIndexOf(".");
    node.setSelectionRange(0, dot > 0 ? dot : node.value.length);
    node.scrollIntoView({ block: "nearest" });
  }

  function onNameKey(event: KeyboardEvent) {
    // The box sits inside a row, and the row has shortcuts of its own: Space
    // must type a space here, not fold the folder.
    event.stopPropagation();
    const input = event.currentTarget as HTMLInputElement;
    if (event.key === "Enter") settle(input.value.trim() || null);
    else if (event.key === "Escape") settle(null);
  }
</script>

<aside style="width: {width}px" class:collapsed class:resizing>
  <!-- Collapsed, the sidebar is an empty rail: the tree fades out rather than
       shrinking into a column of icons. `inert` takes it out of the tab order
       and away from the pointer while it is not there to be seen. -->
  <div class="content" inert={collapsed}>
    {#if items.length === 0 && empty}
      <p class="empty">{empty}</p>
    {/if}

    <!-- The space below the last row belongs to the tree too: right-clicking
         it is how something is made at the top level. A click on a row never
         reaches here; the row stops it. -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <ul
      bind:this={list}
      oncontextmenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        oncontext(e, null);
      }}
    >
      {#each items as item (item.key)}
        {@const naming = item.editing !== undefined}
        <li>
          <div
            class="row"
            class:active={item.key === activeKey}
            class:selected={item.key === selectedKey}
            role="button"
            tabindex="0"
            title={naming ? undefined : item.path}
            aria-expanded={item.folder ? item.folder === "open" : undefined}
            onclick={() => !naming && onselect(item)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") onselect(item);
              else onkey(e, item);
            }}
            oncontextmenu={(e) => {
              // Stopped, not merely defaulted: the window-level handler in App
              // opens the plain menu, and a row may want one of its own.
              e.preventDefault();
              e.stopPropagation();
              if (!naming) oncontext(e, item);
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
            {#if naming}
              <input
                class="name"
                value={item.editing}
                spellcheck="false"
                autocomplete="off"
                aria-label="Name"
                use:nameBox
                onkeydown={onNameKey}
                onclick={(e) => e.stopPropagation()}
                onblur={(e) => settle(e.currentTarget.value.trim() || null)}
              />
            {:else}
              <span class="label">{item.title}</span>
              {#if item.dirty}
                <span class="dirty" aria-label="Unsaved changes"></span>
              {/if}
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

  .row:hover,
  .row.selected {
    background: var(--hover);
  }
  .row:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }

  /* After `.selected`, so the file being edited keeps its tint when it is
     also the row last clicked -- which it usually is. */
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

  /* Sized and set like the label it stands in for, so a row does not jump
     when it turns into a box and back. */
  .name {
    flex: 1;
    min-width: 0;
    height: 18px;
    padding: 0 0.25rem;
    margin-left: -0.25rem;
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: 3px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.82rem;
    outline: none;
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
