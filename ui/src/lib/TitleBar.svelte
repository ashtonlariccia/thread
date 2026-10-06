<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { canEdit, runEdit, type EditCommand } from "./edit";
  import {
    connectionLabel,
    type RemoteStatus,
    type SavedConnection,
  } from "./state/remote.svelte";

  type Props = {
    /** Whether a file is open, so entries that need one can be disabled. */
    hasFile: boolean;
    /** The file being edited, shown in the middle of the bar; null for none. */
    path: string | null;
    /** How many folders are open in the file tree. */
    folderCount: number;
    onnew: () => void;
    onopen: () => void;
    onopenfolder: () => void;
    onclosefolder: () => void;
    onsave: () => void;
    onsaveas: () => void;
    onclosefile: () => void;
    sidebarCollapsed: boolean;
    ontogglesidebar: () => void;
    /** Terminal -> New Terminal: a shell in a tab of its own. */
    onnewterminal: () => void;
    /** Whether the window is on this machine, or on a remote. */
    remoteStatus: RemoteStatus;
    /** The saved connections, for Remote -> Connect Known. */
    known: SavedConnection[];
    /** Remote -> Connect: ask who to connect to. */
    onconnect: () => void;
    onconnectknown: (id: string) => void;
    /** Stop saving a connection. */
    onforgetknown: (id: string) => void;
    ondisconnect: () => void;
    onnewwindow: () => void;
    onappearance: () => void;
    /** This window only. */
    onclosewindow: () => void;
    /** File -> Exit: the whole application. */
    onquit: () => void;
  };

  let {
    hasFile,
    path,
    folderCount,
    onnew,
    onopen,
    onopenfolder,
    onclosefolder,
    onsave,
    onsaveas,
    onclosefile,
    sidebarCollapsed,
    ontogglesidebar,
    onnewterminal,
    remoteStatus,
    known,
    onconnect,
    onconnectknown,
    onforgetknown,
    ondisconnect,
    onnewwindow,
    onappearance,
    onclosewindow,
    onquit,
  }: Props = $props();

  type MenuName = "file" | "edit" | "terminal" | "remote";

  let openMenu = $state<MenuName | null>(null);
  /** Whether the list under Remote -> Connect Known is out. */
  let knownOpen = $state(false);
  // One connection at a time: a window on a remote leaves it before joining another.
  const canConnect = $derived(remoteStatus === "local");
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
    knownOpen = false;
  }

  function show(menu: MenuName | null) {
    // Sampled as the menu opens: it is the answer to "what would these act on".
    if (menu === "edit") editable = canEdit();
    knownOpen = false;
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
          <button class="menu-item" role="menuitem" onclick={() => run(onnew)}>
            <span>New File</span>
            <span class="hint">Ctrl+N</span>
          </button>
          <button class="menu-item" role="menuitem" onclick={() => run(onopen)}>
            <span>Open File…</span>
            <span class="hint">Ctrl+O</span>
          </button>
          <button class="menu-item" role="menuitem" onclick={() => run(onopenfolder)}>
            <span>Open Folder…</span>
            <span class="hint">Ctrl+Shift+O</span>
          </button>

          <div class="sep"></div>

          <button class="menu-item" role="menuitem" disabled={!hasFile} onclick={() => run(onsave)}>
            <span>Save</span>
            <span class="hint">Ctrl+S</span>
          </button>
          <button
            class="menu-item"
            role="menuitem"
            disabled={!hasFile}
            onclick={() => run(onsaveas)}
          >
            <span>Save As…</span>
            <span class="hint">Ctrl+Shift+S</span>
          </button>
          <button
            class="menu-item"
            role="menuitem"
            disabled={!hasFile}
            onclick={() => run(onclosefile)}
          >
            <span>Close File</span>
            <span class="hint">Ctrl+W</span>
          </button>
          <button
            class="menu-item"
            role="menuitem"
            disabled={folderCount === 0}
            onclick={() => run(onclosefolder)}
          >
            <!-- One folder is closed from here; one of several is closed by
                 right-clicking it in the tree. -->
            {folderCount > 1 ? "Close All Folders" : "Close Folder"}
          </button>

          <div class="sep"></div>

          <button class="menu-item" role="menuitem" onclick={() => run(onnewwindow)}>
            New Window
          </button>

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

    <!-- Terminal -->
    <div class="menu-host">
      <button
        class="menu-trigger"
        class:open={openMenu === "terminal"}
        onclick={() => toggle("terminal")}
        onmouseenter={() => hover("terminal")}
      >
        Terminal
      </button>

      {#if openMenu === "terminal"}
        <div class="menu" role="menu">
          <button class="menu-item" role="menuitem" onclick={() => run(onnewterminal)}>
            <span>New Terminal</span>
            <span class="hint">Ctrl+Shift+`</span>
          </button>
        </div>
      {/if}
    </div>

    <!-- Remote -->
    <div class="menu-host">
      <button
        class="menu-trigger"
        class:open={openMenu === "remote"}
        onclick={() => toggle("remote")}
        onmouseenter={() => hover("remote")}
      >
        Remote
      </button>

      {#if openMenu === "remote"}
        <div class="menu" role="menu">
          <button
            class="menu-item"
            role="menuitem"
            disabled={!canConnect}
            onclick={() => run(onconnect)}
          >
            Connect…
          </button>

          <!-- The saved connections fly out beside the entry, as a submenu
               does: one move to the right, and the one wanted is under the
               pointer. -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="sub-host"
            onmouseenter={() => (knownOpen = true)}
            onmouseleave={() => (knownOpen = false)}
          >
            <button
              class="menu-item"
              role="menuitem"
              aria-haspopup="menu"
              aria-expanded={knownOpen}
              disabled={!canConnect || known.length === 0}
              onclick={() => (knownOpen = !knownOpen)}
            >
              <span>Connect Known</span>
              <span class="hint">▸</span>
            </button>

            {#if knownOpen && canConnect && known.length > 0}
              <div class="menu sub" role="menu">
                {#each known as connection (connection.id)}
                  <div class="known">
                    <button
                      class="menu-item"
                      role="menuitem"
                      onclick={() => run(() => onconnectknown(connection.id))}
                    >
                      {connectionLabel(connection)}
                    </button>
                    <button
                      class="forget"
                      title="Forget this connection"
                      aria-label="Forget {connectionLabel(connection)}"
                      onclick={() => run(() => onforgetknown(connection.id))}
                    >
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
            {/if}
          </div>

          <div class="sep"></div>

          <button
            class="menu-item"
            role="menuitem"
            disabled={remoteStatus !== "connected" && remoteStatus !== "lost"}
            onclick={() => run(ondisconnect)}
          >
            Disconnect
          </button>
        </div>
      {/if}
    </div>
  </nav>

  <!-- Grows to fill the middle, so most of the bar stays draggable. -->
  <div class="drag" data-tauri-drag-region></div>

  <!-- Centred on the window, not on the gap between the menus and the
       buttons: those are different widths, and centring between them would
       sit visibly off the middle. It takes no pointer events, so the bar
       under it is still what gets dragged. -->
  {#if path}
    <!-- The left-to-right mark keeps a path that starts with punctuation
         (`\\server\share`) reading in order inside the right-to-left box the
         truncation needs; see `.path`. -->
    <div class="path">&lrm;{path}</div>
  {/if}

  <div class="controls">
    <button
      class="ctl"
      onclick={ontogglesidebar}
      aria-expanded={!sidebarCollapsed}
      title={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
      aria-label={sidebarCollapsed ? "Show sidebar" : "Hide sidebar"}
    >
      <!-- A window with its side panel marked off. The same whether the
           sidebar is open or not: the sidebar itself says which. -->
      <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true">
        <rect
          x="1.6"
          y="2.6"
          width="12.8"
          height="10.8"
          rx="2"
          fill="none"
          stroke="currentColor"
          stroke-width="1.2"
        />
        <path d="M6.2 2.6 V13.4" stroke="currentColor" stroke-width="1.2" />
      </svg>
    </button>
    <span class="gap"></span>

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

  /* A path too long for its slot loses its *start*: the drive and the first
     few folders are the part you already know, and the end is the file. An
     ellipsis only ever lands on the overflowing end of a box, so the box is
     right-to-left -- which moves that end to the left without reordering the
     text inside it. */
  .path {
    position: absolute;
    left: 50%;
    top: var(--viewport-inset);
    bottom: 0;
    transform: translateX(-50%);
    max-width: 46%;
    /* Centred by its line height rather than by flexbox: the text has to
       stay one plain box for the ellipsis to be able to clip it. */
    line-height: calc(var(--titlebar-height) - var(--viewport-inset));
    color: var(--fg-dim);
    font-size: 0.72rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    pointer-events: none;
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
  /* Sets the sidebar toggle apart from the window buttons: beside them, but
     not one of them. */
  .gap {
    width: 8px;
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

  /* A submenu hangs off the entry that opens it, level with it. */
  .sub-host {
    position: relative;
  }
  .menu.sub {
    top: calc(-0.2rem - 1px);
    left: 100%;
    min-width: 170px;
  }

  /* A saved connection, and beside it the way to stop saving it: there on
     the row you are pointing at, so a list of them is not a list of buttons. */
  .known {
    display: flex;
    align-items: center;
  }
  .known .menu-item {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
  }
  .forget {
    flex: none;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    margin-left: 2px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg-dim);
    cursor: pointer;
    visibility: hidden;
  }
  .known:hover .forget,
  .forget:focus-visible {
    visibility: visible;
  }
  .forget:hover {
    background: #f38ba81f;
    color: var(--danger);
  }

  .sep {
    height: 1px;
    margin: 0.2rem 0.3rem;
    background: var(--border);
  }
</style>
