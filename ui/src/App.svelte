<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import AppearanceDialog from "./lib/AppearanceDialog.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import Editor from "./lib/Editor.svelte";
  import PinBar from "./lib/PinBar.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SidebarResizer from "./lib/SidebarResizer.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import UnsavedDialog from "./lib/UnsavedDialog.svelte";

  import { item, SEP, type ContextMenuState } from "./lib/contextMenu";
  import { setEditor } from "./lib/edit";
  import { languageOf } from "./lib/languages";
  import { RAIL_WIDTH } from "./lib/layout";
  import { AppearanceStore } from "./lib/state/appearance.svelte";
  import { Documents } from "./lib/state/documents.svelte";
  import { fileIcon, loadIcons } from "./lib/state/icons.svelte";
  import { Pins } from "./lib/state/pins.svelte";
  import type { Pin, SidebarItem } from "./lib/types";

  const pins = new Pins();
  const appearance = new AppearanceStore();
  const docs = new Documents();

  const appWindow = getCurrentWindow();

  let appearanceOpen = $state(false);

  // --- sidebars ---------------------------------------------------------------
  //
  // Left is the file tree, which does not exist yet, so it has nothing to
  // list. Right is the files open in this window.
  //
  // Each `*Width` stays the *expanded* width while collapsed, so expanding
  // returns to the width you dragged rather than a default. Both start as
  // rails: an empty list is a quarter of the window spent on nothing.

  let leftWidth = $state(230);
  let leftCollapsed = $state(true);
  let rightWidth = $state(230);
  let rightCollapsed = $state(true);
  let resizing = $state(false);

  const treeItems: SidebarItem[] = [];

  const openFiles = $derived(
    docs.list.map(
      (doc): SidebarItem => ({
        key: doc.key,
        title: doc.name,
        detail: doc.path,
        icon: fileIcon(doc.name),
        dirty: doc.dirty,
      }),
    ),
  );

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

  /** File → Exit. Closes every window; each asks about its own unsaved files. */
  async function quit() {
    try {
      await invoke("quit_app");
    } catch (e) {
      console.error("quit_app failed", e);
    }
  }

  /**
   * Close this window and nothing else: with two windows open, the other one
   * carries on. Goes through the close request below, the same as Alt+F4.
   */
  function closeWindow() {
    void appWindow.close();
  }

  // Ctrl+C/X/V/Z/A are the editor's and the text fields' own; these are the
  // ones the File menu advertises. `preventDefault` matters as much as the
  // handler: the webview has its own ideas about Ctrl+O and Ctrl+S.
  function onKeydown(event: KeyboardEvent) {
    if (!event.ctrlKey || event.altKey || event.metaKey) return;
    const key = event.key.toLowerCase();
    if (key !== "o" && key !== "s" && key !== "w") return;
    event.preventDefault();

    // A dialog is up; the window behind it is not taking commands.
    if (docs.asking || appearanceOpen || event.repeat) return;

    if (key === "o") void docs.openDialog();
    else if (key === "w") void docs.close();
    else if (event.shiftKey) void docs.saveAs();
    else void docs.save();
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

  /** Reload the frontend. The open files live only in this page, so ask first. */
  async function refreshPage() {
    if (await docs.confirm()) location.reload();
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

  /** A row in the open-files list. */
  function onFileContextMenu(event: MouseEvent, row: SidebarItem) {
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        item("Save", () => void docs.save(row.key)),
        item("Save As…", () => void docs.saveAs(row.key)),
        item("Close", () => void docs.close(row.key)),
        SEP,
        item("Refresh Page", refreshPage),
      ],
    };
  }

  // Drives every surface in app.css at once, so opacity is one number in one
  // place rather than a rule per pane. Set on the root element because the
  // whole cascade reads it, including components this file never touches.
  $effect(() => {
    document.documentElement.style.setProperty("--bg-alpha", String(appearance.alpha));
  });

  onMount(() => {
    setEditor(docs.editor);

    void (async () => {
      await Promise.all([pins.refresh(), appearance.load()]);

      // Liveness beacon: proof in the backend's log that the frontend came up.
      void invoke("ui_ready", {
        detail: `APP_READY pins=${pins.list.length} scale=${appearance.current.scale}`,
      }).catch(() => {});

      // Not needed until a file is open, so not paid for before the window is up.
      loadIcons();

      // `thread.exe some-file`, or "Open with" from Explorer.
      const startup = await invoke<string[]>("startup_files").catch(() => []);
      for (const path of startup) await docs.open(path);
    })();

    // The × in the bar, Alt+F4, the taskbar's Close and File → Exit all arrive
    // here, so unsaved files are asked about however the window is shut.
    const stopClose = appWindow.onCloseRequested(async (event) => {
      if (!(await docs.confirm())) event.preventDefault();
    });

    return () => {
      setEditor(null);
      void stopClose.then((unlisten) => unlisten());
    };
  });
</script>

<svelte:window onkeydown={onKeydown} oncontextmenu={onWindowContextMenu} />

<div class="app">
  <TitleBar
    hasFile={docs.active !== null}
    onopen={() => void docs.openDialog()}
    onsave={() => void docs.save()}
    onsaveas={() => void docs.saveAs()}
    onclosefile={() => void docs.close()}
    {leftCollapsed}
    {rightCollapsed}
    ontoggleleft={() => (leftCollapsed = !leftCollapsed)}
    ontoggleright={() => (rightCollapsed = !rightCollapsed)}
    onnewwindow={newWindow}
    onappearance={() => (appearanceOpen = true)}
    onclosewindow={closeWindow}
    onquit={quit}
  />

  <main class:resizing>
    <Sidebar
      side="left"
      items={treeItems}
      activeKey={null}
      width={leftCollapsed ? RAIL_WIDTH : leftWidth}
      collapsed={leftCollapsed}
      {resizing}
      onselect={() => {}}
      onclose={() => {}}
      oncontext={() => {}}
    />

    <!-- No handle while collapsed: the rail has one width, and a drag that
         silently expanded it would fight the toggle in the title bar. -->
    {#if !leftCollapsed}
      <SidebarResizer
        side="left"
        width={leftWidth}
        onresize={(w) => (leftWidth = w)}
        ondragging={(d) => (resizing = d)}
      />
    {/if}

    <!-- A resizer normally provides the gap on its side; where a sidebar is
         collapsed it is not rendered, so the stage supplies its own. -->
    <section class="stage" class:railed-left={leftCollapsed} class:railed-right={rightCollapsed}>
      <Editor host={docs.editor} />
      {#if docs.list.length === 0}
        <p class="empty">Open a file with Ctrl+O</p>
      {/if}
    </section>

    {#if !rightCollapsed}
      <SidebarResizer
        side="right"
        width={rightWidth}
        onresize={(w) => (rightWidth = w)}
        ondragging={(d) => (resizing = d)}
      />
    {/if}

    <Sidebar
      side="right"
      items={openFiles}
      activeKey={docs.activeKey}
      width={rightCollapsed ? RAIL_WIDTH : rightWidth}
      collapsed={rightCollapsed}
      {resizing}
      onselect={(key) => docs.select(key)}
      onclose={(key) => void docs.close(key)}
      oncontext={onFileContextMenu}
    />
  </main>

  {#if ctx}
    <ContextMenu x={ctx.x} y={ctx.y} items={ctx.items} onclose={() => (ctx = null)} />
  {/if}

  <PinBar
    pins={pins.list}
    onopen={openPin}
    onunpin={(pin) => void pins.remove(pin)}
    onmove={(pin, index) => void pins.move(pin, index)}
  >
    {#snippet info()}
      {#if docs.active}
        <span class="info-path" title={docs.active.path}>{docs.active.path}</span>
        <span>Ln {docs.cursor.line}, Col {docs.cursor.col}</span>
        <span>{docs.active.eol === "crlf" ? "CRLF" : "LF"}</span>
        <span>{docs.active.bom ? "UTF-8 with BOM" : "UTF-8"}</span>
        <span>{languageOf(docs.active.name)}</span>
      {/if}
    {/snippet}
  </PinBar>

  <AppearanceDialog
    open={appearanceOpen}
    {appearance}
    onclose={() => (appearanceOpen = false)}
  />

  <UnsavedDialog docs={docs.asking?.docs ?? null} onanswer={(choice) => void docs.answer(choice)} />
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
     the same trick a browser plays with its content area.

     It is also the painted surface for the editor, which draws no background
     of its own: one layer here is what keeps the window a single opacity. */
  .stage {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    margin: var(--viewport-inset) 0;
    background: var(--bg-viewport-wash);
    border: 1px solid var(--border);
    border-radius: var(--viewport-radius);
    transition: margin 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
    /* Keeps the content inside the rounded corners. Safe here, unlike on the
       sidebars: nothing in the stage needs to escape it. */
    overflow: hidden;
  }

  .stage.railed-left {
    margin-left: var(--viewport-inset);
  }
  .stage.railed-right {
    margin-right: var(--viewport-inset);
  }

  @media (prefers-reduced-motion: reduce) {
    .stage {
      transition: none;
    }
  }

  /* Over the blank editor, and out of the way of anything aimed at it. */
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    color: var(--fg-faint);
    font-size: 0.8rem;
    user-select: none;
    pointer-events: none;
  }

  /* The path is the one part of the status that can be any length, so it is
     the part that gives way. */
  .info-path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
