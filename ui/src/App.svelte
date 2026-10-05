<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { message } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import AppearanceDialog from "./lib/AppearanceDialog.svelte";
  import ChangedDialog from "./lib/ChangedDialog.svelte";
  import ConfirmDialog, { type Confirmation } from "./lib/ConfirmDialog.svelte";
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
  import { indentLabel } from "./lib/indent";
  import { languageOf } from "./lib/languages";
  import { RAIL_WIDTH } from "./lib/layout";
  import { ConfigStore, type Config } from "./lib/state/config.svelte";
  import { Documents } from "./lib/state/documents.svelte";
  import { dirName, pathKey, samePath, segmentsBelow } from "./lib/paths";
  import { fileIcon, folderIcon, loadFolderIcons, loadIcons } from "./lib/state/icons.svelte";
  import { Pins } from "./lib/state/pins.svelte";
  import { Tree } from "./lib/state/tree.svelte";
  import type { Pin, SidebarItem } from "./lib/types";

  const pins = new Pins();
  const config = new ConfigStore();
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
  // starts as a rail: an empty list is a quarter of the window spent on
  // nothing. Collapsed it shows nothing at all -- the tree fades out with it.

  let sidebarWidth = $state(230);
  let sidebarCollapsed = $state(true);
  let resizing = $state(false);

  /** The files with unsaved changes, by [`pathKey`], for marking in the tree. */
  const dirtyPaths = $derived(
    new Set(docs.dirty.flatMap((doc) => (doc.path === null ? [] : [pathKey(doc.path)]))),
  );

  /** The row last clicked or right-clicked. */
  let selectedKey = $state<string | null>(null);

  /**
   * A name being typed into the tree. `row` is the row being renamed, or for
   * something new the folder row it will appear under.
   */
  let naming = $state<{ kind: "file" | "folder" | "rename"; row: string; dir: string } | null>(
    null,
  );

  /** Stands in for the entry being created until it has a name and exists. */
  const NEW_ROW = "\0new";

  const treeItems = $derived.by(() => {
    const items = tree.rows.map(
      ({ id, entry, depth, open, root }): SidebarItem => ({
        key: id,
        path: entry.path,
        title: entry.name,
        icon: entry.dir ? folderIcon(entry.name, open, root) : fileIcon(entry.name),
        depth,
        folder: entry.dir ? (open ? "open" : "closed") : undefined,
        root,
        dirty: !entry.dir && dirtyPaths.has(pathKey(entry.path)),
        editing: naming?.kind === "rename" && naming.row === id ? entry.name : undefined,
      }),
    );

    if (naming && naming.kind !== "rename") {
      // First inside its folder, above what is already there: where the eye
      // already is, and where it will not be hidden below a long listing.
      const parent = items.findIndex((row) => row.key === naming!.row);
      if (parent !== -1) {
        items.splice(parent + 1, 0, {
          key: NEW_ROW,
          path: "",
          title: "",
          icon: naming.kind === "file" ? fileIcon("") : folderIcon("", false),
          depth: items[parent].depth + 1,
          folder: naming.kind === "folder" ? "closed" : undefined,
          root: false,
          dirty: false,
          editing: "",
        });
      }
    }
    return items;
  });

  /** The tree row for the file being edited, so the two stay visibly in step. */
  const activeTreeKey = $derived.by(() => {
    const path = docs.active?.path;
    if (!path) return null;
    return treeItems.find((row) => !row.folder && samePath(row.path, path))?.key ?? null;
  });

  // Whatever file is being edited is shown in the tree, unfolding down to it
  // if need be -- so opening one from a dialog or Ctrl+Tab still says where in
  // the project it lives. Only when the *file* changes: folding its folder
  // away afterwards is the user's business.
  $effect(() => {
    const path = docs.active?.path;
    if (path && tree.roots.length > 0) void tree.reveal(path);
  });

  /**
   * Close folders, and with them the files open from inside them. A file
   * stays if it is also inside a folder that is staying open.
   *
   * The files go first: if there are unsaved ones and the question about them
   * is cancelled, the folders stay open too, rather than leaving tabs behind
   * for a project that has gone from the tree.
   */
  async function closeFolders(paths: string[]) {
    const closing = (path: string) => paths.some((root) => segmentsBelow(root, path) !== null);
    const staying = tree.roots.map((root) => root.path).filter((root) => !paths.includes(root));
    const kept = (path: string) => staying.some((root) => segmentsBelow(root, path) !== null);

    if (!(await docs.closeWhere((path) => closing(path) && !kept(path)))) return;
    for (const path of paths) tree.close(path);
  }

  function onTreeSelect(row: SidebarItem) {
    selectedKey = row.key;
    if (row.folder) void tree.toggle(row.path);
    else void docs.open(row.path);
  }

  // --- creating, renaming, deleting ---------------------------------------------

  /** The root a row belongs to: the first half of its key. */
  const rootOf = (key: string) => key.slice(0, key.indexOf("\n"));
  const rowKey = (root: string, path: string) => `${root}\n${path}`;

  /**
   * The folder row something new should go under: the folder that was
   * right-clicked, the folder of the file that was, or — for the empty space
   * below the tree — the first folder that is open.
   */
  function creationTarget(from: SidebarItem | null): { row: string; dir: string } | null {
    const row = from ?? treeItems[0];
    if (!row) return null;
    if (row.folder) return { row: row.key, dir: row.path };
    const dir = dirName(row.path);
    return { row: rowKey(rootOf(row.key), dir), dir };
  }

  async function startNew(kind: "file" | "folder", from: SidebarItem | null) {
    const target = creationTarget(from);
    if (!target) return;
    // Unfolded first: the name box is drawn among the folder's children.
    await tree.expand(target.dir);
    naming = { kind, ...target };
  }

  function startRename(row: SidebarItem) {
    // An opened folder is addressed by its path everywhere; renaming it here
    // would pull the tree out from under itself.
    if (row.root) return;
    naming = { kind: "rename", row: row.key, dir: dirName(row.path) };
  }

  /** The name box was settled, with a name or without one. */
  async function finishNaming(value: string | null) {
    const pending = naming;
    naming = null;
    if (!pending || value === null) return;

    try {
      if (pending.kind === "rename") {
        const row = treeItems.find((r) => r.key === pending.row);
        if (!row || value === row.title) return;
        const renamed = await invoke<string>("rename_path", { path: row.path, name: value });
        docs.renamed(row.path, renamed);
        await tree.reload(pending.dir);
        selectedKey = rowKey(rootOf(pending.row), renamed);
      } else {
        const command = pending.kind === "file" ? "create_file" : "create_dir";
        const created = await invoke<string>(command, { dir: pending.dir, name: value });
        await tree.reload(pending.dir);
        selectedKey = rowKey(rootOf(pending.row), created);
        // A new file is made to be written in.
        if (pending.kind === "file") await docs.open(created);
      }
    } catch (e) {
      void message(String(e), { title: "Thread", kind: "error" });
    }
  }

  // --- yes-or-no questions ---------------------------------------------------------
  //
  // Asked in the app's own dialog. The system's message box is the wrong
  // size, the wrong colours and the wrong font for this window, and cannot be
  // told which button is the dangerous one.

  let question = $state<{ ask: Confirmation; resolve: (confirmed: boolean) => void } | null>(null);

  function confirm(ask: Confirmation): Promise<boolean> {
    // One at a time; a second question while one is up is a "no".
    if (question) return Promise.resolve(false);
    return new Promise((resolve) => (question = { ask, resolve }));
  }

  function answerQuestion(confirmed: boolean) {
    const asked = question;
    question = null;
    asked?.resolve(confirmed);
  }

  async function deleteRow(row: SidebarItem) {
    if (row.root) return;
    const confirmed = await confirm({
      title: row.folder ? "Delete folder" : "Delete file",
      message: row.folder
        ? `Delete the folder "${row.title}" and everything in it?`
        : `Delete "${row.title}"?`,
      note: "It goes to the Recycle Bin, and can be restored from there.",
      confirm: "Delete",
      danger: true,
    });
    if (!confirmed) return;

    try {
      await invoke("delete_path", { path: row.path });
      if (selectedKey === row.key) selectedKey = null;
      await tree.reload(dirName(row.path));
      // Any open copy is now the only one; the same check that notices a file
      // deleted from outside marks it unsaved.
      void docs.checkDisk();
    } catch (e) {
      void message(String(e), { title: "Thread", kind: "error" });
    }
  }

  /** The shortcuts a focused tree row answers to. */
  function onTreeKey(event: KeyboardEvent, row: SidebarItem) {
    if (event.key === "F2") {
      event.preventDefault();
      startRename(row);
    } else if (event.key === "Delete") {
      event.preventDefault();
      void deleteRow(row);
    }
  }

  /**
   * File → Open Folder. Adds to whatever folders are already open: nothing is
   * swapped out, and a folder stays until it is closed.
   */
  async function openFolder(path?: string) {
    loadFolderIcons();
    const before = tree.roots.length;
    await (path === undefined ? tree.openDialog() : tree.open(path));
    // A folder was asked for; show it. Not if the dialog was cancelled.
    if (tree.roots.length > before || path !== undefined) sidebarCollapsed = false;
  }

  // --- session ------------------------------------------------------------------
  //
  // What is open is written down as it changes and put back at the next
  // launch. Only the main window does either: a session is one window's worth
  // of state, and two windows taking turns to overwrite it would restore
  // whichever happened to write last.

  type Session = {
    folders: string[];
    unfolded: string[];
    files: string[];
    active: string | null;
    sidebarCollapsed: boolean;
    sidebarWidth: number;
  };

  const keepsSession = appWindow.label === "main";
  /** Nothing is saved until the last session is back: half of it is not a session. */
  let sessionRestored = $state(false);

  async function restoreSession() {
    const session = await invoke<Session>("session_load");
    sidebarWidth = session.sidebarWidth;

    if (session.folders.length > 0) loadFolderIcons();
    await tree.restore(session.folders, session.unfolded);
    for (const path of session.files) await docs.open(path, { quiet: true });

    const active = docs.list.find((d) => d.path !== null && d.path === session.active);
    if (active) docs.select(active.key);
    // Last, so the tree is already there when the sidebar opens onto it.
    sidebarCollapsed = session.sidebarCollapsed;
  }

  const session = $derived<Session>({
    folders: tree.roots.map((root) => root.path),
    unfolded: tree.unfolded,
    files: docs.list.flatMap((doc) => (doc.path === null ? [] : [doc.path])),
    active: docs.active?.path ?? null,
    sidebarCollapsed,
    sidebarWidth: Math.round(sidebarWidth),
  });

  $effect(() => {
    if (!keepsSession || !sessionRestored) return;
    const snapshot = $state.snapshot(session);
    // Dragging the sidebar or opening a folder changes this many times in a
    // row; one write once it settles is enough.
    const timer = setTimeout(() => {
      invoke("session_save", { session: snapshot }).catch((e) =>
        console.error("session_save failed", e),
      );
    }, 400);
    return () => clearTimeout(timer);
  });

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
  //
  // Handled on the way *down* to the editor and stopped there, so they mean
  // the same thing whatever the editor would have made of them. Vim has uses
  // of its own for Ctrl+O, Ctrl+W and Ctrl+N; a key that both closed the file
  // and started a vim window command would be worse than either.
  const SHORTCUTS = new Set(["n", "o", "s", "w", "tab"]);

  function onKeydown(event: KeyboardEvent) {
    if (!event.ctrlKey || event.altKey || event.metaKey) return;
    const key = event.key.toLowerCase();
    if (!SHORTCUTS.has(key)) return;
    event.preventDefault();
    event.stopPropagation();

    // A dialog is up; the window behind it is not taking commands.
    if (docs.busy || appearanceOpen || question) return;

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

  /**
   * A row in the file tree, or the empty space below the rows (`row` null).
   * This menu is the only way to create, rename or delete from the tree.
   */
  function onTreeContextMenu(event: MouseEvent, row: SidebarItem | null) {
    if (row) selectedKey = row.key;

    // With no folder open there is nowhere to make anything.
    const create =
      tree.roots.length > 0
        ? [
            item("New File", () => void startNew("file", row)),
            item("New Folder", () => void startNew("folder", row)),
            SEP,
          ]
        : [];
    // An opened folder is closed, not renamed or deleted, from here.
    const own = !row
      ? []
      : row.root
        ? [item("Close Folder", () => void closeFolders([row.path])), SEP]
        : [
            item("Rename", () => startRename(row)),
            item("Delete", () => void deleteRow(row), true),
            SEP,
          ];

    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [...create, ...own, item("Refresh Page", refreshPage)],
    };
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
    document.documentElement.style.setProperty("--bg-alpha", String(config.alpha));
  });

  // The editor's share of the config: the font, the gutter, and each open
  // file's indentation. Re-run whenever the config is replaced.
  $effect(() => {
    const current = config.current;
    // Only the config is a dependency: `configure` walks the open files, and
    // must not re-run just because one was opened or closed.
    untrack(() => docs.configure(current));
  });

  /** A new config has arrived, from this window's dialog, another's, or the file. */
  function onConfigChanged(next: Config) {
    const exclude = JSON.stringify(config.current.files.exclude);
    config.current = next;
    // What the tree leaves out is decided where it is listed, so the listings
    // on screen are only as current as the last time they were read.
    if (JSON.stringify(next.files.exclude) !== exclude) void tree.refresh();
  }

  function checkDisk() {
    void docs.checkDisk();
    void tree.poll();
  }

  onMount(() => {
    setEditor(docs.editor);

    void (async () => {
      await Promise.all([pins.refresh(), config.load()]);

      // Liveness beacon: proof in the backend's log that the frontend came up.
      void invoke("ui_ready", {
        detail: `APP_READY pins=${pins.list.length} scale=${config.appearance.scale}`,
      }).catch(() => {});

      // Not needed until a file is open, so not paid for before the window is up.
      loadIcons();

      // `thread.exe some-file`, `thread.exe .`, or "Open with" from Explorer.
      const startup = await invoke<{ path: string; dir: boolean }[]>("startup_files").catch(
        () => [],
      );
      for (const { path, dir } of startup) await (dir ? openFolder(path) : docs.open(path));

      // Asked to open something specific: do that, and leave the last session
      // where it is. Otherwise carry on from where things were left.
      if (keepsSession) {
        if (startup.length === 0) await restoreSession().catch((e) => console.error(e));
        sessionRestored = true;
      }
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

    // The config is shared by every window and can be changed from any of
    // them, or by saving the config file, so each window is told.
    const stopConfig = listen<Config>("config-changed", (event) => onConfigChanged(event.payload));

    // The config file was saved with something in it that cannot be used.
    // Nothing has changed; say what is wrong while the file is still open.
    const stopConfigError = listen<string>("config-error", (event) => {
      void message(event.payload, { title: "Config not applied", kind: "error" });
    });

    return () => {
      setEditor(null);
      clearInterval(diskPoll);
      void stopConfig.then((unlisten) => unlisten());
      void stopConfigError.then((unlisten) => unlisten());
      void stopClose.then((unlisten) => unlisten());
    };
  });
</script>

<!-- Coming back to the window is when a stale file would be noticed, so that
     moment does not wait for the next tick of the poll. -->
<svelte:window
  onkeydowncapture={onKeydown}
  oncontextmenu={onWindowContextMenu}
  onfocus={checkDisk}
/>

<div class="app">
  <TitleBar
    hasFile={docs.active !== null}
    path={docs.active ? (docs.active.path ?? docs.active.name) : null}
    folderCount={tree.roots.length}
    onnew={() => docs.newFile()}
    onopen={() => void docs.openDialog()}
    onopenfolder={() => void openFolder()}
    onclosefolder={() => void closeFolders(tree.roots.map((root) => root.path))}
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
      {selectedKey}
      width={sidebarCollapsed ? RAIL_WIDTH : sidebarWidth}
      collapsed={sidebarCollapsed}
      {resizing}
      empty="No folder open. File → Open Folder, or Ctrl+Shift+O."
      onselect={onTreeSelect}
      onkey={onTreeKey}
      onedit={(value) => void finishNaming(value)}
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
    {#snippet start()}
      {#if docs.vimMode && docs.active}
        <span class="mode" data-mode={docs.vimMode}>{docs.vimMode}</span>
      {/if}
      <!-- Vim's `:` line, `/` search and messages are put here by `vim.ts`,
           beside the mode, where vim itself shows them. Always present, so
           there is somewhere to put them the moment vim asks. -->
      <span class="vim-line" data-vim-line bind:this={docs.vimLine}></span>
    {/snippet}
    {#snippet info()}
      {#if docs.active}
        <span>Ln {docs.cursor.line}, Col {docs.cursor.col}</span>
        <span>{indentLabel(docs.active.indent)}</span>
        <span>{docs.active.eol === "crlf" ? "CRLF" : "LF"}</span>
        <span>{docs.active.bom ? "UTF-8 with BOM" : "UTF-8"}</span>
        <span>{languageOf(docs.active.name)}</span>
      {/if}
    {/snippet}
  </PinBar>

  <AppearanceDialog
    open={appearanceOpen}
    {config}
    onclose={() => (appearanceOpen = false)}
  />

  <UnsavedDialog docs={docs.asking?.docs ?? null} onanswer={(choice) => void docs.answer(choice)} />

  <ConfirmDialog question={question?.ask ?? null} onanswer={answerQuestion} />

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

  /* What goes in here is built by the vim extension, not by this component,
     so it is reached with `:global`. Its own inline styles ask for a default
     monospace font and, for messages, a hard red. */
  .vim-line {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    overflow: hidden;
  }
  /* Everything the extension puts in here takes the bar's own font, size and
     colour, over the monospace and the colours it asks for inline -- so the
     left-hand end of the bar reads as the same strip of status as the right. */
  .vim-line :global(*) {
    color: inherit !important;
    font-family: inherit !important;
    font-size: inherit;
  }
  .vim-line :global(input) {
    min-width: 0;
    padding: 0;
    background: transparent;
    border: none;
    outline: none;
  }

  /* The mode, set like the status items across the bar from it. It is named
     the way they are too: "Normal", not vim's shouted "NORMAL". */
  .mode {
    text-transform: capitalize;
  }

  /* Vim's end of the bar is the one part of it that is typed into and that
     changes what the next key does, so it takes the accent: same font and
     size as the status opposite, but not something to hunt for among it. */
  .mode,
  .vim-line {
    color: var(--accent);
  }
  .vim-line :global(.cm-vim-message) {
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
