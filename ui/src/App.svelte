<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import AppearanceDialog from "./lib/AppearanceDialog.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import PinBar from "./lib/PinBar.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SidebarResizer from "./lib/SidebarResizer.svelte";
  import TitleBar from "./lib/TitleBar.svelte";

  import { item, SEP, type ContextMenuState } from "./lib/contextMenu";
  import { RAIL_WIDTH } from "./lib/layout";
  import { AppearanceStore } from "./lib/state/appearance.svelte";
  import { Pins } from "./lib/state/pins.svelte";
  import type { Pin, SidebarItem } from "./lib/types";

  const pins = new Pins();
  const appearance = new AppearanceStore();

  const appWindow = getCurrentWindow();

  let appearanceOpen = $state(false);

  // Nothing fills the sidebar yet; the list, selection and close are wired so
  // the first thing that does only has to push rows.
  let items = $state<SidebarItem[]>([]);
  let activeKey = $state<number | null>(null);

  function closeItem(key: number) {
    items = items.filter((i) => i.key !== key);
    if (activeKey === key) activeKey = items.at(-1)?.key ?? null;
  }

  // `sidebarWidth` stays the *expanded* width while collapsed, so expanding
  // returns to the width you dragged rather than a default.
  let sidebarWidth = $state(230);
  // Starts as the rail: 230px of empty list is a quarter of the window spent
  // on nothing until something is open.
  let sidebarCollapsed = $state(true);
  let resizing = $state(false);

  const effectiveSidebarWidth = $derived(sidebarCollapsed ? RAIL_WIDTH : sidebarWidth);

  // --- pins -------------------------------------------------------------------

  /** Click on the strip. Nothing produces pins yet, so nothing opens them. */
  function openPin(_pin: Pin) {}

  // --- menu actions ---------------------------------------------------------

  async function newWindow() {
    try {
      await invoke("new_window");
    } catch (e) {
      console.error("new_window failed", e);
    }
  }

  async function quit() {
    try {
      await invoke("quit_app");
    } catch (e) {
      console.error("quit_app failed", e);
    }
  }

  /**
   * Close this window and nothing else: with two windows open, the other one
   * carries on. Only File → Exit means the whole application.
   */
  function closeWindow() {
    void appWindow.close();
  }

  // --- right-click menu -------------------------------------------------------
  //
  // WebView2 supplies its own, and it is a *browser's* menu: a dozen entries
  // about pages, history and printing, none of which mean anything here. It is
  // suppressed everywhere the app has something better to say -- which is
  // everywhere except a text field, where Cut/Copy/Paste is the native menu
  // earning its keep.
  //
  // One menu, owned here, so a right-click on a sidebar row and a right-click
  // on the window cannot both leave a popover up.

  let ctx = $state<ContextMenuState | null>(null);

  function refreshPage() {
    location.reload();
  }

  /** True for anything where the browser's own Cut/Copy/Paste menu is right. */
  function isEditable(target: EventTarget | null): boolean {
    return (
      target instanceof HTMLElement &&
      (target.isContentEditable || !!target.closest("input, textarea"))
    );
  }

  function onWindowContextMenu(event: MouseEvent) {
    // Something nearer the click already answered it -- the pinned strip has
    // its own menu.
    if (event.defaultPrevented || isEditable(event.target)) return;
    event.preventDefault();
    ctx = { x: event.clientX, y: event.clientY, items: [item("Refresh Page", refreshPage)] };
  }

  /** A row in the sidebar: the same menu, plus the one thing a row can do. */
  function onItemContextMenu(event: MouseEvent, row: SidebarItem) {
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [item("Close", () => closeItem(row.key)), SEP, item("Refresh Page", refreshPage)],
    };
  }

  // Drives every surface in app.css at once, so opacity is one number in one
  // place rather than a rule per pane. Set on the root element because the
  // whole cascade reads it, including components this file never touches.
  $effect(() => {
    document.documentElement.style.setProperty("--bg-alpha", String(appearance.alpha));
  });

  onMount(() => {
    void (async () => {
      await Promise.all([pins.refresh(), appearance.load()]);

      // Liveness beacon: proof in the backend's log that the frontend came up.
      void invoke("ui_ready", {
        detail: `APP_READY pins=${pins.list.length} scale=${appearance.current.scale}`,
      }).catch(() => {});
    })();
  });
</script>

<svelte:window oncontextmenu={onWindowContextMenu} />

<div class="app">
  <TitleBar
    onnewwindow={newWindow}
    onappearance={() => (appearanceOpen = true)}
    onclosewindow={closeWindow}
    onquit={quit}
  />

  <main class:resizing>
    <Sidebar
      {items}
      {activeKey}
      width={effectiveSidebarWidth}
      collapsed={sidebarCollapsed}
      {resizing}
      onselect={(key) => (activeKey = key)}
      onclose={closeItem}
      ontoggle={() => (sidebarCollapsed = !sidebarCollapsed)}
      oncontext={onItemContextMenu}
    />

    <!-- No handle while collapsed: the rail has one width, and a drag that
         silently expanded it would fight the toggle. -->
    {#if !sidebarCollapsed}
      <SidebarResizer
        width={sidebarWidth}
        onresize={(w) => (sidebarWidth = w)}
        ondragging={(d) => (resizing = d)}
      />
    {/if}

    <!-- The resizer normally provides the gap on this side; collapsed, it is
         not rendered, so the stage supplies its own. -->
    <section class="stage" class:railed={sidebarCollapsed}></section>
  </main>

  {#if ctx}
    <ContextMenu x={ctx.x} y={ctx.y} items={ctx.items} onclose={() => (ctx = null)} />
  {/if}

  <PinBar
    pins={pins.list}
    onopen={openPin}
    onunpin={(pin) => void pins.remove(pin)}
    onmove={(pin, index) => void pins.move(pin, index)}
  />

  <AppearanceDialog
    open={appearanceOpen}
    {appearance}
    onclose={() => (appearanceOpen = false)}
  />
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    /* The whole window's surface, including the inset around the viewport. */
    background: var(--bg-chrome);
  }

  main {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* While dragging, stop the stage from selecting text under the cursor and
     keep the resize cursor even when the pointer strays off the handle. */
  main.resizing {
    cursor: col-resize;
    user-select: none;
  }

  /* The one bordered thing in the window. The chrome around it is seamless, so
     this line is what separates "the app" from "what the app is showing" --
     the same trick a browser plays with its content area. */
  .stage {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    margin: var(--viewport-inset) var(--viewport-inset) var(--viewport-inset) 0;
    background: var(--bg-viewport-wash);
    border: 1px solid var(--border);
    border-radius: var(--viewport-radius);
    transition: margin-left 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
    /* Keeps the content inside the rounded corners. Safe here, unlike on the
       sidebar: nothing in the stage needs to escape it. */
    overflow: hidden;
  }

  .stage.railed {
    margin-left: var(--viewport-inset);
  }

  @media (prefers-reduced-motion: reduce) {
    .stage {
      transition: none;
    }
  }
</style>
