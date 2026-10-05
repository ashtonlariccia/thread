<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { canEdit, runEdit, type EditCommand } from "./edit";

  type Props = {
    onnewwindow: () => void;
    onappearance: () => void;
    /** This window only. */
    onclosewindow: () => void;
    /** File -> Exit: the whole application. */
    onquit: () => void;
  };

  let { onnewwindow, onappearance, onclosewindow, onquit }: Props = $props();

  type MenuName = "file" | "edit";

  let openMenu = $state<MenuName | null>(null);
  let menuWrap: HTMLElement | undefined;
  let maximized = $state(false);
  /** Whether a text field had focus when the menu opened. */
  let editable = $state(false);

  const appWindow = getCurrentWindow();

  const EDITS: { command: EditCommand; label: string; hint: string; sepBefore?: boolean }[] = [
    { command: "undo", label: "Undo", hint: "Ctrl+Z" },
    { command: "redo", label: "Redo", hint: "Ctrl+Y" },
    { command: "cut", label: "Cut", hint: "Ctrl+X", sepBefore: true },
    { command: "copy", label: "Copy", hint: "Ctrl+C" },
    { command: "paste", label: "Paste", hint: "Ctrl+V" },
    { command: "selectAll", label: "Select All", hint: "Ctrl+A", sepBefore: true },
  ];

  function closeMenus() {
    openMenu = null;
  }

  function show(menu: MenuName | null) {
    // Sampled as the menu opens: it is the answer to "what would these act on".
    if (menu === "edit") editable = canEdit();
    openMenu = menu;
  }

  function toggle(menu: MenuName) {
    show(openMenu === menu ? null : menu);
  }

  /** Menu bars track the pointer once open: hovering a sibling switches to it. */
  function hover(menu: MenuName) {
    if (openMenu !== null && openMenu !== menu) show(menu);
  }

  /** Every entry closes the menu first, so no item has to remember to. */
  function run(action: () => void) {
    closeMenus();
    action();
  }

  // Close on outside click or Escape, the way a native menu behaves. Testing
  // containment (rather than stopping propagation inside the menu) keeps the
  // markup free of handlers that exist only to block bubbling.
  function onWindowClick(event: MouseEvent) {
    if (menuWrap && event.target instanceof Node && menuWrap.contains(event.target)) return;
    closeMenus();
  }
  function onWindowKey(event: KeyboardEvent) {
    if (event.key === "Escape") closeMenus();
  }

  async function toggleMaximize() {
    await appWindow.toggleMaximize();
    maximized = await appWindow.isMaximized();
  }

  $effect(() => {
    void appWindow.isMaximized().then((v) => (maximized = v));
  });
</script>

<svelte:window onclick={onWindowClick} onkeydown={onWindowKey} />

<!-- data-tauri-drag-region makes the empty areas behave like a real titlebar. -->
<header class="titlebar" data-tauri-drag-region>
  <!-- A press on the menus must not take focus: the Edit entries act on the
       focused text field, and a menu that stole focus would leave them nothing
       to act on. Clicks still land. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <nav class="menus" bind:this={menuWrap} onmousedown={(e) => e.preventDefault()}>
    <!-- File -->
    <div class="menu-host">
      <button
        class="menu-trigger"
        class:open={openMenu === "file"}
        onclick={() => toggle("file")}
        onmouseenter={() => hover("file")}
      >
        File
      </button>

      {#if openMenu === "file"}
        <div class="menu" role="menu">
          <button class="menu-item" role="menuitem" onclick={() => run(onnewwindow)}>
            New Window
          </button>

          <div class="sep"></div>

          <button class="menu-item" role="menuitem" onclick={() => run(onclosewindow)}>
            Close Window
          </button>
          <button class="menu-item" role="menuitem" onclick={() => run(onquit)}>Exit</button>
        </div>
      {/if}
    </div>

    <!-- Edit -->
    <div class="menu-host">
      <button
        class="menu-trigger"
        class:open={openMenu === "edit"}
        onclick={() => toggle("edit")}
        onmouseenter={() => hover("edit")}
      >
        Edit
      </button>

      {#if openMenu === "edit"}
        <div class="menu" role="menu">
          {#each EDITS as edit (edit.command)}
            {#if edit.sepBefore}
              <div class="sep"></div>
            {/if}
            <button
              class="menu-item"
              role="menuitem"
              disabled={!editable}
              onclick={() => run(() => void runEdit(edit.command))}
            >
              <span>{edit.label}</span>
              <span class="hint">{edit.hint}</span>
            </button>
          {/each}

          <div class="sep"></div>

          <button class="menu-item" role="menuitem" onclick={() => run(onappearance)}>
            Appearance…
          </button>
        </div>
      {/if}
    </div>
  </nav>

  <!-- Grows to fill the middle, so most of the bar stays draggable. -->
  <div class="drag" data-tauri-drag-region></div>

  <div class="controls">
    <button class="ctl" title="Minimize" onclick={() => appWindow.minimize()}>
      <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0 5 h10" stroke="currentColor" stroke-width="1.2" />
      </svg>
    </button>

    <button class="ctl" title={maximized ? "Restore" : "Maximize"} onclick={toggleMaximize}>
      {#if maximized}
        <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="0.5" y="2.5" width="7" height="7" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.1" />
          <path d="M2.6 2.4 V1.6 A1 1 0 0 1 3.6 0.6 H8.5 A1 1 0 0 1 9.5 1.6 V6.5 A1 1 0 0 1 8.5 7.5 H7.6" fill="none" stroke="currentColor" stroke-width="1.1" />
        </svg>
      {:else}
        <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="0.6" y="0.6" width="8.8" height="8.8" rx="1.4" fill="none" stroke="currentColor" stroke-width="1.1" />
        </svg>
      {/if}
    </button>

    <!-- Closes this window, not the application: with two windows open, the
         other one carries on. File -> Exit is the one that ends everything. -->
    <button class="ctl close" title="Close window" onclick={onclosewindow}>
      <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0.6 0.6 L9.4 9.4 M9.4 0.6 L0.6 9.4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
      </svg>
    </button>
  </div>
</header>

<style>
  .titlebar {
    /* Every chip in the bar is sized from this rather than from a repeated
       literal, so the menus and the window buttons stay the same height and
       centre on the same line. */
    --titlebar-height: 28px;

    display: flex;
    align-items: stretch;
    height: var(--titlebar-height);
    flex: none;
    /* The eye does not centre the menus in the 28px bar -- it centres them in
       the whole band between the window's top edge and the viewport below,
       which is the bar plus the viewport's inset. Padding the top by that inset
       hands the extra space to the top, so the chips land on the band's centre
       line instead of riding high above it. */
    padding-top: var(--viewport-inset);
    /* Less the pill's own padding and margin, so the label -- not the invisible
       pill -- starts on the shared text line. */
    padding-left: calc(var(--bar-text-inset) - 0.7rem - 1px);
    user-select: none;
    /* Above the stage, so menus are never painted over. */
    position: relative;
    z-index: 100;
  }

  .menus {
    display: flex;
    align-items: center;
  }

  .menu-host {
    position: relative;
    display: flex;
    align-items: center;
  }

  /* Hover chips float inside the bar instead of spanning its full height, and
     are fully rounded into pills. Height is the bar less the chip inset top and
     bottom -- stated rather than left to the text's own line box, so the pill
     is centred on the bar exactly and matches the window buttons beside it. The
     label is then centred inside the pill by the flexbox, not by padding. */
  .menu-trigger {
    display: flex;
    align-items: center;
    height: calc(var(--titlebar-height) - 2 * var(--chip-inset));
    background: transparent;
    border: none;
    border-radius: 999px;
    color: var(--fg);
    cursor: pointer;
    font-size: 0.75rem;
    line-height: 1;
    /* Horizontal room the pill needs on its ends, or the rounded caps crowd the
       text -- plus a hair at the top. `align-items: center` centres the line
       box, and that box reserves descender depth these labels never use, so
       the letters sit about 0.7px high; top padding shifts content by half its
       value, hence 1.4px. Measured off a screenshot, not guessed. */
    padding: 1.4px 0.7rem 0;
    margin: 0 1px;
    transition: background 90ms ease;
  }
  .menu-trigger:hover {
    background: var(--hover);
  }
  .menu-trigger.open {
    background: var(--hover-strong);
  }

  .drag {
    flex: 1;
    min-width: 0;
  }

  .controls {
    display: flex;
    align-items: center;
    flex: none;
    padding-right: 2px;
  }

  /* Rounded and inset, so they read as buttons rather than as slabs welded to
     the window edge. Same height as the menu pills, from the same measurement. */
  .ctl {
    width: 30px;
    height: calc(var(--titlebar-height) - 2 * var(--chip-inset));
    background: transparent;
    border: none;
    border-radius: var(--chip-radius);
    color: var(--fg-dim);
    cursor: pointer;
    display: grid;
    place-items: center;
    margin: 0 1px;
  }
  .ctl:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .ctl.close:hover {
    background: #f38ba8;
    color: var(--accent-ink);
  }

  /* Menus float above everything, including the stage. */
  .menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 1000;
    min-width: 190px;
    padding: 0.2rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 5px;
    box-shadow: 0 8px 24px #0009;
  }

  .menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1.25rem;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg);
    cursor: pointer;
    font-size: 0.8rem;
    padding: 0.35rem 0.5rem;
  }
  .menu-item:hover:not(:disabled) {
    background: var(--hover);
    color: var(--accent);
  }
  .menu-item:disabled {
    color: var(--fg-faint);
    cursor: default;
  }

  .hint {
    color: var(--fg-dim);
    font-size: 0.72rem;
  }
  .menu-item:disabled .hint {
    color: var(--fg-faint);
  }

  .sep {
    height: 1px;
    margin: 0.2rem 0.3rem;
    background: var(--border);
  }
</style>
