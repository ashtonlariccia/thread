<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import AppearanceDialog from "./lib/AppearanceDialog.svelte";
  import ChangedDialog from "./lib/ChangedDialog.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import Editor from "./lib/Editor.svelte";
  import PinBar from "./lib/PinBar.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SidebarResizer from "./lib/SidebarResizer.svelte";
  import TabBar, { type Tab } from "./lib/TabBar.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import UnsavedDialog from "./lib/UnsavedDialog.svelte";

  import { item, SEP, type ContextMenuState } from "./lib/contextMenu";
  import { setEditor } from "./lib/edit";
  import { languageOf } from "./lib/languages";
  import { RAIL_WIDTH } from "./lib/layout";
  import { AppearanceStore, type Appearance } from "./lib/state/appearance.svelte";
  import { Documents } from "./lib/state/documents.svelte";
  import { samePath } from "./lib/paths";
  import { fileIcon, folderIcon, loadFolderIcons, loadIcons } from "./lib/state/icons.svelte";
  import { Pins } from "./lib/state/pins.svelte";
  import { Tree, type Entry } from "./lib/state/tree.svelte";
  import type { Pin, SidebarItem, SidebarKey } from "./lib/types";

  const pins = new Pins();
  const appearance = new AppearanceStore();
  const docs = new Documents();
  const tree = new Tree();

  const appWindow = getCurrentWindow();

  /** How often open files and unfolded folders are compared with the disk. */
  const DISK_POLL_MS = 1000;

  let appearanceOpen = $state(false);

  // --- sidebar ----------------------------------------------------------------
  //
  // The file tree. `sidebarWidth` stays the *expanded* width while collapsed,
  // so expanding returns to the width you dragged rather than a default. It
  // starts as a rail: an empty list is a quarter of the window spent on nothing.

  let sidebarWidth = $state(230);
  let sidebarCollapsed = $state(true);
  let resizing = $state(false);

  function treeItem(entry: Entry, depth: number, open: boolean, root: boolean): SidebarItem {
    return {
      key: entry.path,
      title: entry.name,
      detail: entry.path,
      icon: entry.dir ? folderIcon(entry.name, open, root) : fileIcon(entry.name),
      depth,
      folder: entry.dir ? (open ? "open" : "closed") : undefined,
    };
  }

  // Unfolded, the whole tree. As a rail there is no room for nesting, so it
  // shows one level -- what is directly inside the project -- as icons: enough
  // to jump to a top-level file, or to a folder (which unfolds the sidebar).
  const treeItems = $derived(
    sidebarCollapsed
      ? tree.topLevel.map((entry) => treeItem(entry, 0, tree.isOpen(entry.path), false))
      : tree.rows.map((row) => treeItem(row.entry, row.depth, row.open, row.root)),
  );

  /** The tree row for the file being edited, so the two stay visibly in step. */
  const activeTreeKey = $derived.by(() => {
    const path = docs.active?.path;
    if (!path) return null;
    return treeItems.find((row) => samePath(row.key, path))?.key ?? null;
  });

  // Whatever file is being edited is shown in the tree, unfolding down to it
  // if need be -- so opening one from a dialog or Ctrl+Tab still says where in
  // the project it lives. Only when the *file* changes: folding its folder
  // away afterwards is the user's business.
  $effect(() => {
    const path = docs.active?.path;
    if (path && tree.root) void tree.reveal(path);
  });

  function onTreeSelect(key: SidebarKey) {
    const path = key;
    const row = treeItems.find((r) => r.key === key);
    if (!row?.folder) {
      void docs.open(path);
    } else if (sidebarCollapsed) {
      // On the rail a folder cannot unfold in place; open the sidebar to it.
      sidebarCollapsed = false;
      void tree.expand(path);
    } else {
      void tree.toggle(path);
    }
  }

  async function openFolder(path?: string) {
    loadFolderIcons();
    await (path === undefined ? tree.openDialog() : tree.open(path));
    // A folder was asked for; show it.
    if (tree.root) sidebarCollapsed = false;
  }

  // --- tabs -------------------------------------------------------------------

  const tabs = $derived(
    docs.list.map(
      (doc): Tab => ({
        key: doc.key,
        name: doc.name,
        detail: doc.path ?? "Not saved yet",
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
  // ones the File menu advertises, plus Ctrl+Tab. `preventDefault` matters as
  // much as the handler: the webview has its own ideas about most of them.
  const SHORTCUTS = new Set(["n", "o", "s", "w", "tab"]);

  function onKeydown(event: KeyboardEvent) {
    if (!event.ctrlKey || event.altKey || event.metaKey) return;
    const key = event.key.toLowerCase();
    if (!SHORTCUTS.has(key)) return;
    event.preventDefault();

    // A dialog is up; the window behind it is not taking commands.
    if (docs.busy || appearanceOpen) return;

    if (key === "tab") {
      docs.cycle(event.shiftKey ? -1 : 1);
      return;
    }
    // Holding the others down should not open a stack of dialogs or files.
    if (event.repeat) return;

    if (key === "n") docs.newFile();
    else if (key === "o" && event.shiftKey) void openFolder();
    else if (key === "o") void docs.openDialog();
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

  /** A row in the file tree. Nothing of its own to offer yet. */
  function onTreeContextMenu(event: MouseEvent) {
    ctx = { x: event.clientX, y: event.clientY, items: [item("Refresh Page", refreshPage)] };
  }

  /** A tab. */
  function onTabContextMenu(event: MouseEvent, key: number) {
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        item("Save", () => void docs.save(key)),
        item("Save As…", () => void docs.saveAs(key)),
        item("Close", () => void docs.close(key)),
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

  function checkDisk() {
    void docs.checkDisk();
    void tree.poll();
  }

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

      // `thread.exe some-file`, `thread.exe .`, or "Open with" from Explorer.
      const startup = await invoke<{ path: string; dir: boolean }[]>("startup_files").catch(
        () => [],
      );
      for (const { path, dir } of startup) await (dir ? openFolder(path) : docs.open(path));
    })();

    // The × in the bar, Alt+F4, the taskbar's Close and File → Exit all arrive
    // here, so unsaved files are asked about however the window is shut.
    const stopClose = appWindow.onCloseRequested(async (event) => {
      if (!(await docs.confirm())) event.preventDefault();
    });

    // Files change on disk while the window is in the background as often as
    // not -- that is when other tools are running -- so this does not wait for
    // focus. It is one `stat` per open file.
    const diskPoll = setInterval(checkDisk, DISK_POLL_MS);

    // The appearance is shared by every window and can be changed from any of
    // them, or by saving the settings file, so each window is told.
    const stopAppearance = listen<Appearance>("appearance-changed", (event) => {
      appearance.current = event.payload;
    });

    // The settings file was saved. What the tree leaves out comes from it.
    const stopSettings = listen("settings-changed", () => void tree.refresh());

    return () => {
      setEditor(null);
      clearInterval(diskPoll);
      void stopAppearance.then((unlisten) => unlisten());
      void stopSettings.then((unlisten) => unlisten());
      void stopClose.then((unlisten) => unlisten());
    };
  });
</script>

<!-- Coming back to the window is when a stale file would be noticed, so that
     moment does not wait for the next tick of the poll. -->
<svelte:window
  onkeydown={onKeydown}
  oncontextmenu={onWindowContextMenu}
  onfocus={checkDisk}
/>

<div class="app">
  <TitleBar
    hasFile={docs.active !== null}
    hasFolder={tree.root !== null}
    onnew={() => docs.newFile()}
    onopen={() => void docs.openDialog()}
    onopenfolder={() => void openFolder()}
    onclosefolder={() => tree.close()}
    onsave={() => void docs.save()}
    onsaveas={() => void docs.saveAs()}
    onclosefile={() => void docs.close()}
    {sidebarCollapsed}
    ontogglesidebar={() => (sidebarCollapsed = !sidebarCollapsed)}
    onnewwindow={newWindow}
    onappearance={() => (appearanceOpen = true)}
    onclosewindow={closeWindow}
    onquit={quit}
  />

  <main class:resizing>
    <Sidebar
      items={treeItems}
      activeKey={activeTreeKey}
      width={sidebarCollapsed ? RAIL_WIDTH : sidebarWidth}
      collapsed={sidebarCollapsed}
      {resizing}
      tree
      empty="No folder open. File → Open Folder, or Ctrl+Shift+O."
      onselect={onTreeSelect}
      oncontext={onTreeContextMenu}
    />

    <!-- No handle while collapsed: the rail has one width, and a drag that
         silently expanded it would fight the toggle in the title bar. -->
    {#if !sidebarCollapsed}
      <SidebarResizer
        width={sidebarWidth}
        onresize={(w) => (sidebarWidth = w)}
        ondragging={(d) => (resizing = d)}
      />
    {/if}

    <!-- The resizer normally provides the gap on this side; collapsed, it is
         not rendered, so the stage supplies its own. -->
    <section class="stage" class:railed={sidebarCollapsed}>
      <!-- No strip with nothing open: an empty bar across the top of an empty
           editor is a line with no reason to be there. -->
      {#if tabs.length > 0}
        <TabBar
          {tabs}
          activeKey={docs.activeKey}
          onselect={(key) => docs.select(key)}
          onclose={(key) => void docs.close(key)}
          oncontext={onTabContextMenu}
        />
      {/if}

      <div class="editor-area">
        <Editor host={docs.editor} />
        {#if docs.list.length === 0}
          <p class="empty">Ctrl+O to open a file, Ctrl+N for a new one</p>
        {/if}
      </div>
    </section>
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
        <span class="info-path" title={docs.active.path}>
          {docs.active.path ?? docs.active.name}
        </span>
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

  <ChangedDialog doc={docs.changed?.doc ?? null} onanswer={(reload) => docs.answerChanged(reload)} />
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

     It is also the painted surface for the tabs and the editor, which draw no
     background of their own: one layer here is what keeps the window a single
     opacity. */
  .stage {
    display: flex;
    flex-direction: column;
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

  /* Whatever the tab strip leaves. The editor fills it absolutely, so it is
     the positioning context for that and for the empty-state hint. */
  .editor-area {
    position: relative;
    flex: 1;
    min-height: 0;
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
