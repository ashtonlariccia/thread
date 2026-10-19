<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { message } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";

  import AppearanceDialog from "./lib/AppearanceDialog.svelte";
  import ChangedDialog from "./lib/ChangedDialog.svelte";
  import CommandPalette, { type Command } from "./lib/CommandPalette.svelte";
  import ConfirmDialog, { type Confirmation } from "./lib/ConfirmDialog.svelte";
  import ConnectDialog from "./lib/ConnectDialog.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import Dialog from "./lib/Dialog.svelte";
  import LspDialog from "./lib/LspDialog.svelte";
  import Pane from "./lib/Pane.svelte";
  import PinBar from "./lib/PinBar.svelte";
  import RemoteBrowseDialog, { type BrowseRequest } from "./lib/RemoteBrowseDialog.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SidebarResizer from "./lib/SidebarResizer.svelte";
  import type { Tab } from "./lib/TabBar.svelte";
  import TerminalView from "./lib/TerminalView.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import TmuxSidebar from "./lib/tmux/TmuxSidebar.svelte";
  import TmuxView from "./lib/tmux/TmuxView.svelte";
  import UnsavedDialog from "./lib/UnsavedDialog.svelte";
  import WorkflowBar from "./lib/WorkflowBar.svelte";

  import { applyAccent } from "./lib/accent";
  import { applyTheme } from "./lib/themes";
  import { item, SEP, type ContextMenuState } from "./lib/contextMenu";
  import { runEdit, setEditor, type EditCommand } from "./lib/edit";
  import { iconUrl } from "./lib/icons";
  import { RAIL_WIDTH } from "./lib/layout";
  import type { Direction, Divider, Rect, SavedNode, SplitDir } from "./lib/panes";
  import { resolveLink, type Link } from "./lib/links";
  import type { OpenKind } from "./lib/terminal";
  import { ConfigStore, type Config } from "./lib/state/config.svelte";
  import { Documents } from "./lib/state/documents.svelte";
  import { Lsp } from "./lib/state/lsp.svelte";
  import { tabDrag } from "./lib/state/tabDrag.svelte";
  import { baseName, dirName, pathKey, samePath, segmentsBelow } from "./lib/paths";
  import { setRemotePicker, type RemotePicker } from "./lib/pick";
  import { fileIcon, folderIcon, loadFolderIcons, loadIcons } from "./lib/state/icons.svelte";
  import { Pins } from "./lib/state/pins.svelte";
  import {
    connectionLabel,
    RemoteStore,
    type ConnectError,
    type ConnectRequest,
    type DiscoveredKey,
    type RemoteInfo,
    type SavedConnection,
  } from "./lib/state/remote.svelte";
  import { Workflows, type Workflow } from "./lib/state/workflows.svelte";
  import {
    busy,
    jobs,
    Tmux,
    Watch,
    type Job,
    type PaneNow,
    type Process,
    type TmuxPane,
  } from "./lib/tmux/tmux.svelte";
  import type { Pin, SidebarItem } from "./lib/types";

  const pins = new Pins();
  const config = new ConfigStore();
  const flows = new Workflows();
  /** The panes and the file tree of the workflow in front. */
  const layout = $derived(flows.current.layout);
  const tree = $derived(flows.current.tree);
  const docs = new Documents({
    get current() {
      return flows.current.layout;
    },
    get all() {
      return flows.layouts;
    },
  });
  const remote = new RemoteStore();
  const lsp = new Lsp(docs, {
    roots: () => tree.roots.map((root) => root.path),
    ready: () => remote.status === "local" || remote.status === "connected",
  });
  docs.watcher = lsp;
  const tmux = new Tmux();
  /** What has happened in the tmux windows that are not being looked at. */
  const watch = new Watch();

  const appWindow = getCurrentWindow();

  /**
   * How often open files and unfolded folders are compared with the disk:
   * each a `stat`, so often enough that the tree keeps up with a terminal.
   */
  const DISK_POLL_MS = 250;

  let appearanceOpen = $state(false);
  let lspOpen = $state(false);
  let paletteOpen = $state(false);

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

  /**
   * The row last clicked or right-clicked, for as long as the tree is what
   * is being worked in: the mark goes when anything else is.
   */
  let selectedKey = $state<string | null>(null);

  /** A press or the focus has landed somewhere: outside the tree, it lets go of its row. */
  function onElsewhere(event: Event) {
    if (selectedKey === null || naming !== null || !(event.target instanceof Element)) return;
    // The menu a row opened is still about that row.
    if (event.target.closest("[data-sidebar], .ctx") === null) selectedKey = null;
  }

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
  // if need be -- so opening one from a dialog or a tab still says where in
  // the project it lives. Only when the *file* changes: folding its folder
  // away afterwards is the user's business.
  $effect(() => {
    const path = docs.active?.path;
    // Untracked: `reveal` reads which folders are unfolded as it goes, and
    // watching those too would have this unfold the file's folder again the
    // moment it was folded.
    if (path && tree.roots.length > 0) untrack(() => void tree.reveal(path));
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

    const files = docs.list.filter((doc) => doc.path !== null && closing(doc.path) && !kept(doc.path));
    if (!(await closeFiles(files.map((doc) => doc.key)))) return;
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
      void message(String(e), { title: "Tide", kind: "error" });
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
      note: remote.remote
        ? "It is deleted from the remote for good; there is no bin there to restore it from."
        : "It goes to the Recycle Bin, and can be restored from there.",
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
      void message(String(e), { title: "Tide", kind: "error" });
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

  // --- terminals ----------------------------------------------------------------
  //
  // A terminal is a tab like a file is, and there can be several. Each is a
  // shell for whatever this window is working on. None exists until one is
  // asked for, and closing its tab ends it: there is no hidden state in
  // between, so a window with no terminal tab is running no shell. The one
  // exception is a remote that keeps them in tmux: there the window closing
  // leaves them running, and its session brings their tabs back.

  type TerminalTab = {
    key: number;
    name: string;
    /**
     * A name no other terminal has, for a remote that keeps terminals to
     * keep this one under: what it is come back to by when the connection
     * has dropped and returned.
     */
    session: string;
    /**
     * Where the shell was last sent: null for the home folder it starts in,
     * and after that wherever Jump Terminal Here has taken it. What is typed
     * at the shell itself is not followed.
     */
    cwd: string | null;
    /** The command line of the shell it runs; null for the one the config names. */
    shell: string | null;
    /** The workflow it belongs to: it is only ever shown in that one's panes. */
    flow: number;
  };

  /** A shell there is to open a terminal with, as the backend lists them. */
  type Shell = { name: string; command: string };
  /** The shells installed where the window is working; none on a remote. */
  let shells = $state.raw<Shell[]>([]);

  let terminals = $state<TerminalTab[]>([]);
  /** Each open terminal's view, by key, for the menus to act on. */
  const terminalViews = $state<Record<number, TerminalView | undefined>>({});

  // Files are keyed upwards from 1 and terminals downwards from -1, so a
  // strip can hold both under one kind of key and tell them apart.
  let nextTerminalKey = -1;
  const isTerminal = (key: number) => key < 0;

  /** The terminals of the workflow in front. */
  const flowTerminals = $derived(terminals.filter((t) => t.flow === flows.current.id));

  /** The terminal showing in the focused pane, or null while a file is. */
  const activeTerminal = $derived.by(() => {
    const active = layout.current.active;
    return active !== null && isTerminal(active) ? active : null;
  });
  /** The one last shown, for Toggle Terminal to come back to. */
  let lastTerminal: number | null = null;
  $effect(() => {
    if (activeTerminal !== null) lastTerminal = activeTerminal;
  });

  /**
   * The file last worked on. With the keyboard in a terminal there is no
   * file in front, and this is the one the window is still about.
   */
  let lastFile = $state<number | null>(null);
  $effect(() => {
    if (docs.activeKey !== null) lastFile = docs.activeKey;
  });

  // The terminal is set in the editor's font, the two being read side by
  // side, at a size and with a cursor of its own.
  const terminalLook = $derived({
    fontFamily: config.current.editor.font_family,
    fontSize: config.current.terminal.font_size,
    cursor: config.current.terminal.cursor,
  });

  /**
   * The folder the window is working in: the open folder, or with none open,
   * the folder of the file being edited. With several folders open it is the one that
   * file is in, and the first of them if it is in none. Not where a shell
   * starts: that is always the home folder.
   */
  function workingDir(): string | null {
    const file = (docs.active ?? docs.find(lastFile))?.path ?? null;
    const roots = tree.roots.map((root) => root.path);
    // The deepest: with a project and one of its subfolders both open, the
    // file belongs to the more specific one.
    const holding = roots
      .filter((root) => file !== null && segmentsBelow(root, file) !== null)
      .sort((a, b) => b.length - a.length)[0];
    return holding ?? roots[0] ?? (file ? dirName(file) : null);
  }

  /**
   * Start a terminal, as a tab that is not yet in any pane. In the shell
   * given, and named for it; with none, the config's.
   */
  function makeTerminal(shell?: Shell, saved?: SavedTerminal): number {
    // The lowest number no terminal of this workflow has, so each starts
    // from the top again once its last has gone.
    const taken = new Set(flowTerminals.map((t) => t.name));
    let number = 1;
    while (taken.has(number === 1 ? "Terminal" : `Terminal ${number}`)) number++;
    const key = nextTerminalKey--;
    terminals.push({
      key,
      name: saved?.name ?? shell?.name ?? (number === 1 ? "Terminal" : `Terminal ${number}`),
      // A saved one goes back to the session it was kept in.
      session: saved?.session ?? `t-${crypto.randomUUID().slice(0, 13)}`,
      // Always the home folder, which is what the backend makes of none.
      cwd: null,
      shell: shell?.command ?? null,
      flow: flows.current.id,
    });
    return key;
  }

  /** Terminal → New Terminal: in the pane the keyboard is in. */
  function newTerminal(shell?: Shell) {
    layout.open(makeTerminal(shell));
  }

  /**
   * A path or a web address in a terminal's output was Ctrl+clicked. A
   * path is opened, at the line it names if it names one; an address goes
   * to the browser.
   */
  function followLink(link: Link, cwd: string | null) {
    if ("url" in link) {
      invoke("open_external", { url: link.url }).catch((e) =>
        message(String(e), { title: "Tide", kind: "error" }),
      );
    } else void docs.openAt(resolveLink(link.path, cwd), link.line, link.col);
  }

  /**
   * `cd` to a folder, as the shell a terminal runs would have it typed.
   * On a remote that is a Unix shell; here it goes by the shell's name.
   */
  function cdLine(dir: string, shell: string | null): string {
    const running = (shell ?? config.current.terminal.shell).toLowerCase();
    if (!remote.remote && /\bcmd(\.exe)?\b/.test(running)) return `cd /d "${dir}"`;
    const posix = remote.remote || /\b(bash|zsh|fish|sh|wsl)(\.exe)?\b/.test(running);
    return `cd '${dir.replaceAll("'", posix ? "'\\''" : "''")}'`;
  }

  /** Jump Terminal Here: send the terminal in front to a folder of the tree. */
  function jumpTerminal(key: number, dir: string) {
    const terminal = terminals.find((t) => t.key === key);
    const view = terminalViews[key];
    if (!terminal || !view) return;
    view.send(`${cdLine(dir, terminal.shell)}\r`);
    terminal.cwd = dir;
    view.focus();
  }

  /** Put the keyboard back in whatever the focused pane is showing. */
  function grabFocus() {
    if (mode === "tmux") {
      const pane = tmux.pane;
      if (pane) tmux.handle(pane.id)?.focus();
      return;
    }
    const active = layout.current.active;
    if (active === null) return;
    if (isTerminal(active)) terminalViews[active]?.focus();
    else docs.editor.focus(layout.focused);
  }

  /** Toggle Terminal: to the terminal and back, opening one if there is none. */
  function toggleTerminal() {
    const leaf = layout.current;
    if (activeTerminal !== null) {
      // Back to the file this pane was on before, or any file it has.
      const before = leaf.last !== null && !isTerminal(leaf.last) ? leaf.last : undefined;
      const back = before ?? leaf.tabs.find((key) => !isTerminal(key));
      if (back !== undefined) layout.open(back, leaf.id);
    } else if (flowTerminals.length > 0) {
      const back = flowTerminals.find((t) => t.key === lastTerminal) ?? flowTerminals.at(-1)!;
      layout.open(back.key);
    } else {
      newTerminal();
    }
  }

  /**
   * Close a terminal's tab, which ends its shell and whatever is running in
   * it. Also where a shell that exits by itself lands.
   */
  function closeTerminal(key: number) {
    terminals = terminals.filter((t) => t.key !== key);
    delete terminalViews[key];
    // Its pane shows the neighbour that slides into its place, in whichever
    // workflow it was: a shell can end while its workflow is not in front.
    for (const each of flows.layouts) each.drop(key);
  }

  /**
   * The `tide` command, run in a terminal: open a file in a tab or a
   * folder in the tree, or with `-c` close whichever of the two a path is.
   */
  function fromTerminal(kind: OpenKind, path: string) {
    if (kind === "dir") void openFolder(path);
    else if (kind === "file") void docs.open(path);
    else void closeFromTerminal(path);
  }

  /**
   * `tide -c`: close whatever a path is. An open folder, with the files
   * open from inside it, as from the menu; a file's tab; or a folder inside
   * an open one, which is its files' tabs closed and the folder folded away.
   * Anything unsaved is asked about first, whichever it is.
   */
  async function closeFromTerminal(path: string) {
    const roots = tree.roots.map((root) => root.path);
    const files = docs.list.flatMap((doc) => (doc.path === null ? [] : [doc.path]));
    let root = roots.find((each) => samePath(each, path));
    let file = files.find((each) => samePath(each, path));

    if (root === undefined && file === undefined) {
      // Not open under that spelling. The command gives a path with its
      // links followed, and a folder reached through one is open under the
      // other name: ask where each really is, and compare those.
      const open = [...roots, ...files];
      const real = await invoke<(string | null)[]>("real_paths", { paths: [path, ...open] }).catch(
        () => [] as (string | null)[],
      );
      const wanted = real[0] ?? null;
      const at = wanted === null ? -1 : real.slice(1).findIndex((each) => each !== null && samePath(each, wanted));
      if (at !== -1 && at < roots.length) root = open[at];
      else if (at !== -1) file = open[at];
    }

    if (root !== undefined) {
      await closeFolders([root]);
      return;
    }
    const doc = file === undefined ? undefined : docs.list.find((each) => each.path === file);
    if (doc) {
      await closeFiles([doc.key]);
      return;
    }
    const under = docs.list.filter((each) => each.path !== null && segmentsBelow(path, each.path) !== null);
    if (
      roots.some((each) => segmentsBelow(each, path) !== null) &&
      (await closeFiles(under.map((each) => each.key)))
    ) {
      tree.fold(path);
    }
  }

  // --- workflows ----------------------------------------------------------------
  //
  // Separate desks in the one window, each with its own panes, file tree and
  // terminals, and one in front at a time. Turning to another ends nothing:
  // the terminals of the one behind go on running, out of sight, and its
  // panes are as they were on coming back. A file open in two is the one
  // buffer, as in two panes; closing it in one leaves it open in the other.

  let workflowBar = $state<WorkflowBar | undefined>();

  /** Whether a tab is in another workflow's panes, besides this one's. */
  const heldElsewhere = (key: number) =>
    flows.list.some((flow) => flow.id !== flows.current.id && flow.layout.holding(key).length > 0);

  /** Take a tab out of every pane of this workflow, and nowhere else. */
  function leaveFlow(key: number) {
    for (const leaf of layout.holding(key)) {
      if (leaf.active === key) docs.vimLines[leaf.id]?.replaceChildren();
      releaseFiles(leaf.id, [key]);
      layout.closeTab(leaf.id, key);
    }
  }

  /**
   * Close files in the workflow in front, asking once about the unsaved
   * ones. A file another workflow also has open only leaves this one's
   * panes. False, having closed nothing, if the question was cancelled.
   */
  async function closeFiles(keys: number[], { force = false } = {}): Promise<boolean> {
    const shared = keys.filter(heldElsewhere);
    const own = docs.list.filter((doc) => keys.includes(doc.key) && !shared.includes(doc.key));
    if (!force && !(await docs.confirm(own.filter((doc) => doc.dirty)))) return false;
    for (const key of shared) leaveFlow(key);
    docs.discard(own.map((doc) => doc.key));
    return true;
  }

  /** The files in this workflow's panes. */
  const flowFiles = () =>
    [...new Set(layout.leaves.flatMap((leaf) => leaf.tabs))].filter((key) => !isTerminal(key));

  /** Bring a workflow to the front, as it was left. */
  function switchFlow(id: number) {
    setMode("ide");
    if (id === flows.current.id) return;
    selectedKey = null;
    naming = null;
    flows.select(id);
    // Its tree was not being watched while it was behind.
    void tree.refresh();
    requestAnimationFrame(grabFocus);
  }

  function stepFlow(by: 1 | -1) {
    const count = flows.list.length;
    if (count > 1) switchFlow(flows.list[(flows.index + by + count) % count].id);
  }

  /** New Workflow: an empty one, in front. */
  function newFlow() {
    switchFlow(flows.add().id);
  }

  /**
   * Close a workflow: its files, asking about unsaved ones, and its
   * terminals, which end. Never the last; that one is the window.
   */
  async function closeFlow(id: number) {
    const flow = flows.find(id);
    if (!flow || flows.list.length < 2) return;
    switchFlow(id);

    const shells = flowTerminals.length;
    if (shells > 0) {
      const one = shells === 1;
      const sure = await confirm({
        title: "Close workflow",
        message: `Close "${flows.label(flow)}"?`,
        note: one
          ? "Its terminal is ended, and whatever is running in it."
          : `Its ${shells} terminals are ended, and whatever is running in them.`,
        confirm: "Close Workflow",
        danger: true,
      });
      if (!sure) return;
    }
    if (!(await closeFiles(flowFiles()))) return;

    for (const terminal of flowTerminals) closeTerminal(terminal.key);
    for (const leaf of layout.leaves) releaseFiles(leaf.id, leaf.tabs);
    flows.remove(id);
    selectedKey = null;
    naming = null;
    void tree.refresh();
    requestAnimationFrame(grabFocus);
  }

  function onFlowContextMenu(event: MouseEvent, flow: Workflow) {
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        item("Rename", () => workflowBar?.rename(flow.id)),
        item("New Workflow", newFlow),
        ...(flows.list.length > 1
          ? [SEP, item("Close Workflow", () => void closeFlow(flow.id), true)]
          : []),
      ],
    };
  }

  // --- remote -------------------------------------------------------------------
  //
  // A window is on this machine or on one remote, never some of each. Going
  // to a remote puts away what was open here and brings out what was last
  // open there; disconnecting does the reverse. The backend decides where a
  // path leads by which of the two the window is on, so nothing below this
  // has to know.

  /** What is open on one machine. */
  type Workspace = {
    folders: string[];
    unfolded: string[];
    files: string[];
    active: string | null;
    /** How the stage was split, and which files were in which pane; null for one pane. */
    layout: SavedNode | null;
    /** The terminals kept on a remote, to go back to. None on this machine. */
    terminals?: SavedTerminal[];
    /**
     * Every workflow, the one in front included; the fields above are that
     * one's too, for a build from before there were workflows.
     */
    workflows?: SavedFlow[];
    /** Which of them was in front. */
    workflow?: number;
  };
  /** One workflow, as it is written down: what it had open, and what it was called. */
  type SavedFlow = Omit<Workspace, "workflows" | "workflow"> & { name: string | null };
  /** One of those terminals: the name it is kept under, and what its tab says. */
  type SavedTerminal = { session: string; name: string };
  const NOTHING: Workspace = { folders: [], unfolded: [], files: [], active: null, layout: null };

  /** What was open on this machine, put away while the window is on a remote. */
  let localStash = $state.raw<Workspace | null>(null);
  /** What was open on each saved connection, by its id, when last on it. */
  let remoteStashes = $state.raw<Record<string, Workspace>>({});

  let connectOpen = $state(false);
  let connectError = $state<string | null>(null);
  let keys = $state.raw<DiscoveredKey[]>([]);

  /** A file or folder being chosen on the remote. */
  let browse = $state.raw<BrowseRequest | null>(null);
  /** The remote folder last chosen from, which is where the next choice starts. */
  let lastBrowsed: string | null = null;

  /** The path a tab is known by once the window has gone: a saved file's. */
  const pathOf = (key: number) => (isTerminal(key) ? null : (docs.find(key)?.path ?? null));

  /** How a terminal's tab is named in a saved layout, which is otherwise paths. */
  const TERMINAL_TAB = "terminal:";

  /**
   * Whether the terminals are written down: only on a remote that may keep
   * them. A shell on this machine ends with the window, and there is nothing
   * to go back to.
   */
  const savesTerminals = () => remote.remote && config.current.terminal.persist;

  /** What a tab is known by in a saved layout: a file's path, or a kept terminal's name. */
  function tabName(key: number): string | null {
    if (!isTerminal(key)) return pathOf(key);
    const terminal = savesTerminals() ? terminals.find((t) => t.key === key) : undefined;
    return terminal ? TERMINAL_TAB + terminal.session : null;
  }

  function captureFlow(flow: Workflow): SavedFlow {
    const panes = flow.layout;
    const active = panes.activeFile;
    return {
      name: flow.name,
      folders: flow.tree.roots.map((root) => root.path),
      unfolded: flow.tree.unfolded,
      // As the strips have them, pane by pane, so tabs dragged into an order
      // come back in it.
      files: [...new Set(panes.leaves.flatMap((leaf) => leaf.tabs.flatMap((key) => pathOf(key) ?? [])))],
      active: active === null ? null : pathOf(active),
      // One pane is what a window has anyway, and needs no writing down.
      layout: panes.leaves.length > 1 ? panes.save(tabName) : null,
      terminals: savesTerminals()
        ? terminals.filter((t) => t.flow === flow.id).map(({ session, name }) => ({ session, name }))
        : [],
    };
  }

  function captureWorkspace(): Workspace {
    const workflows = flows.list.map(captureFlow);
    const { name: _name, ...front } = workflows[flows.index];
    return { ...front, workflows, workflow: flows.index };
  }

  /**
   * Open what a workspace had open, every workflow of it, into a window
   * with nothing open. What has since gone is left out quietly.
   */
  async function restoreWorkspace(workspace: Workspace) {
    const saved: SavedFlow[] = workspace.workflows?.length
      ? workspace.workflows
      : [{ ...workspace, name: null }];
    for (const [index, flow] of saved.entries()) {
      // The empty one a window starts with is the first.
      if (index > 0) flows.select(flows.add().id);
      flows.current.name = flow.name ?? null;
      await restoreFlow(flow);
    }
    flows.select(flows.list[Math.min(workspace.workflow ?? 0, flows.list.length - 1)].id);
  }

  /** Open what one workflow had open, into the one in front. */
  async function restoreFlow(workspace: SavedFlow) {
    if (workspace.folders.length > 0) loadFolderIcons();
    await tree.restore(workspace.folders, workspace.unfolded);
    for (const path of workspace.files) await docs.open(path, { quiet: true });

    // Kept terminals, into the pane there is until the layout says otherwise.
    // Each is opened by the name it was kept under, which goes back to it.
    if (savesTerminals()) {
      for (const saved of workspace.terminals ?? []) {
        if (terminals.some((t) => t.session === saved.session)) continue;
        const pane = layout.current;
        const key = makeTerminal(undefined, saved);
        pane.tabs.push(key);
        pane.active ??= key;
      }
    }

    if (workspace.layout) {
      const keyOf = (name: string) =>
        name.startsWith(TERMINAL_TAB)
          ? (terminals.find((t) => TERMINAL_TAB + t.session === name)?.key ?? null)
          : (docs.list.find((doc) => doc.path !== null && samePath(doc.path, name))?.key ?? null);
      const before = layout.leaves.map((leaf) => ({ id: leaf.id, tabs: [...leaf.tabs] }));
      if (layout.restore(workspace.layout, keyOf, before.flatMap((leaf) => leaf.tabs))) {
        // The panes they were opened into have gone; each file's state
        // waits for whichever pane has it now.
        for (const leaf of before) releaseFiles(leaf.id, leaf.tabs);
      }
    }

    const active = docs.list.find((d) => d.path !== null && d.path === workspace.active);
    if (active && layout.holding(active.key).length > 0) docs.select(active.key);
  }

  /** Close everything, without asking: whoever calls this already has. */
  function clearWorkspace() {
    // Unmounting a terminal is what ends its shell.
    for (const terminal of terminals) delete terminalViews[terminal.key];
    terminals = [];
    lastTerminal = null;
    // Each belongs to the machine the window is leaving.
    lsp.reset();
    docs.clear();
    flows.reset();
    selectedKey = null;
    naming = null;
  }

  function browseRemote(mode: BrowseRequest["mode"], suggested = ""): Promise<string | null> {
    // A file that already lives somewhere is saved from there; anything else
    // starts where the last choice was made, or in the folder that is open.
    const start = suggested.includes("/") ? dirName(suggested) : (lastBrowsed ?? workingDir());
    return new Promise((resolve) => {
      browse = {
        mode,
        start,
        name: baseName(suggested),
        resolve: (path) => {
          browse = null;
          if (path !== null) lastBrowsed = mode === "folder" ? path : dirName(path);
          resolve(path);
        },
      };
    });
  }

  const remotePicker: RemotePicker = {
    files: async () => {
      const path = await browseRemote("file");
      return path === null ? null : [path];
    },
    folder: () => browseRemote("folder"),
    save: (suggested) => browseRemote("save", suggested),
  };

  type Attempt = { info: RemoteInfo } | { error: string } | null;

  /**
   * Make a connection, asking about the host's key if it comes to that.
   * Null means the question was answered "no"; nothing else is wrong.
   */
  async function attempt(
    who: string,
    make: (trustNewKey: boolean) => Promise<RemoteInfo>,
  ): Promise<Attempt> {
    const failure = (e: unknown): ConnectError =>
      e !== null && typeof e === "object" && "message" in e
        ? (e as ConnectError)
        : { kind: "session", message: String(e) };

    const before = remote.status;
    remote.pending = who;
    remote.status = "connecting";
    try {
      try {
        return { info: await make(false) };
      } catch (e) {
        const error = failure(e);
        const changed = error.kind === "hostKeyChanged";
        if (!changed && error.kind !== "unknownHostKey") return { error: error.message };

        // Only a person can say whether this is the machine they meant.
        const trusted = await confirm({
          title: changed ? "The host's key has changed" : "Trust this host?",
          message: error.message,
          note: changed
            ? `It was ${error.expectedFingerprint}, and is now ${error.fingerprint}.`
            : `Its key's fingerprint is ${error.fingerprint}. Trusting it remembers the key, ` +
              "and Tide will refuse to connect if it ever changes.",
          confirm: changed ? "Trust the New Key" : "Trust and Connect",
          danger: changed,
        });
        if (!trusted) return null;

        try {
          return { info: await make(true) };
        } catch (again) {
          return { error: failure(again).message };
        }
      }
    } finally {
      remote.pending = null;
      // Whoever asked decides what a success means; until then, as it was.
      remote.status = before;
    }
  }

  /**
   * Move the window onto a remote it has just connected to: put away what is
   * open here, and bring out what was last open there.
   */
  async function goRemote(info: RemoteInfo, local: Workspace = captureWorkspace()) {
    sessionRestored = false;
    localStash = local;
    clearWorkspace();
    remote.info = info;
    remote.status = "connected";
    setRemotePicker(remotePicker);
    lastBrowsed = null;

    const last = info.saved === null ? undefined : remoteStashes[info.saved];
    if (last) await restoreWorkspace(last);
    sessionRestored = keepsSession;
  }

  /** Remote → Connect. */
  function openConnect() {
    connectError = null;
    connectOpen = true;
    void invoke<DiscoveredKey[]>("remote_keys").then((found) => (keys = found));
  }

  /** The connect dialog was filled in. It stays up until this succeeds. */
  async function connectNew(request: ConnectRequest) {
    connectError = null;
    // Before anything is connected: a save made after would go to the remote.
    if (!(await docs.confirm())) return;

    const result = await attempt(connectionLabel(request), (trustNewKey) =>
      invoke<RemoteInfo>("remote_connect", { target: { ...request, trustNewKey } }),
    );
    if (result === null) return;
    if ("error" in result) {
      connectError = result.error;
      return;
    }

    connectOpen = false;
    await goRemote(result.info);
    await offerToSave();
  }

  /**
   * Once signed in to somewhere new: keep it, or not. Kept, it can be
   * connected to again without asking and is what the next launch comes back
   * to. Not kept, it lasts as long as this window does.
   */
  async function offerToSave() {
    const info = remote.info;
    if (!info || info.saved !== null) return;

    const save = await confirm({
      title: "Save this connection?",
      message: `Keep ${info.label}, so Tide can connect to it again without asking.`,
      note:
        "A saved connection is listed under Remote → Hosts, and Tide reopens it, " +
        "with what you had open on it, the next time it starts. What you signed in with is " +
        "stored encrypted for your Windows account. Not saved, it ends when this window closes.",
      confirm: "Save",
      cancel: "Not Now",
    });
    if (!save) return;

    try {
      const saved = await invoke<SavedConnection>("remote_save");
      if (remote.info) remote.info = { ...remote.info, saved: saved.id };
      await remote.refreshKnown();
    } catch (e) {
      void message(String(e), { title: "Tide", kind: "error" });
    }
  }

  const savedLabel = (id: string) => {
    const saved = remote.known.find((connection) => connection.id === id);
    return saved ? connectionLabel(saved) : "the remote";
  };

  /** Remote → Hosts. */
  async function connectKnown(id: string) {
    if (!(await docs.confirm())) return;

    const result = await attempt(savedLabel(id), (trustNewKey) =>
      invoke<RemoteInfo>("remote_connect_saved", { id, trustNewKey }),
    );
    if (result === null) return;
    if ("error" in result) {
      void message(result.error, { title: `Could not connect to ${savedLabel(id)}`, kind: "error" });
      return;
    }
    await goRemote(result.info);
  }

  async function forgetKnown(id: string) {
    const forget = await confirm({
      title: "Forget connection",
      message: `Forget ${savedLabel(id)}?`,
      note: "What it signs in with is deleted. A window connected to it stays connected.",
      confirm: "Forget",
      danger: true,
    });
    if (!forget) return;

    await remote.forget(id);
    const { [id]: _gone, ...rest } = remoteStashes;
    remoteStashes = rest;
  }

  /** Remote → Disconnect: back to this machine, and what was open on it. */
  async function disconnect() {
    if (!remote.remote) return;
    // While still connected, so that "Save" has somewhere to save to.
    if (!(await docs.confirm())) return;

    sessionRestored = false;
    const id = remote.info?.saved ?? null;
    if (id !== null) remoteStashes = { ...remoteStashes, [id]: captureWorkspace() };
    clearWorkspace();

    await invoke("remote_disconnect").catch((e) => console.error("remote_disconnect failed", e));
    remote.status = "local";
    remote.info = null;
    setRemotePicker(null);

    const back = localStash;
    localStash = null;
    if (back) await restoreWorkspace(back);
    sessionRestored = keepsSession;
  }

  /** Connect again after the connection was lost. Nothing open is touched. */
  async function reconnect() {
    const result = await attempt(remote.info?.label ?? "the remote", () =>
      invoke<RemoteInfo>("remote_reconnect"),
    );
    if (result === null) return;
    if ("error" in result) {
      void message(result.error, { title: "Could not reconnect", kind: "error" });
      return;
    }
    remote.info = result.info;
    // The ones it had went with the connection; they start again on this one.
    lsp.reset();
    remote.status = "connected";
    checkDisk();
  }

  // --- tmux ---------------------------------------------------------------------
  //
  // The window's other face. The editor is one; this is the remote's tmux
  // sessions, drawn as the window's own: a session in the sidebar, each of
  // its windows a tab, each pane a terminal. Turning to it puts nothing of
  // the editor away, and turning back finds it as it was. It is the same
  // connection either way: a window is on one remote, whichever face shows.

  type Mode = "ide" | "tmux";
  const MODE_KEY = "tide.mode";

  let mode = $state<Mode>("ide");
  /** Whether the tmux face has been turned to at all: it is not built before. */
  let tmuxOpened = $state(false);
  /** Its sidebar is the list of sessions, and starts out open. */
  let tmuxSidebarCollapsed = $state(false);
  let tmuxSidebar = $state<TmuxSidebar | undefined>();

  tmux.onerror = (text) => void message(text, { title: "tmux", kind: "error" });

  /**
   * Turn the window to one face or the other. tmux is a remote's: turned to
   * on this machine, it asks which, unless told not to.
   */
  function setMode(next: Mode, ask = true) {
    if (next === mode) return;
    mode = next;
    if (keepsSession) {
      try {
        localStorage.setItem(MODE_KEY, next);
      } catch {
        /* nowhere to keep it; the window starts as the editor next time */
      }
    }
    if (next === "ide") {
      // Once it is laid out again, which is when it can take the keyboard.
      requestAnimationFrame(grabFocus);
      return;
    }
    tmuxOpened = true;
    tmux.enter();
    if (ask && remote.status === "local") openConnect();
  }

  // The store follows the connection: up, dropped, or left.
  $effect(() => {
    const status = remote.status;
    untrack(() => tmux.link(status));
  });

  // A dropped connection is made again without being asked: what outlives
  // it -- a tmux session, and each of the editor's own terminals on a remote
  // that keeps them -- is waiting to be gone back to. Quietly, and for as
  // long as it takes; Reconnect, in the command list, is still there to be
  // told why it is not working.
  $effect(() => {
    if (remote.status !== "lost") return;
    let trying = false;
    const again = async () => {
      if (trying || remote.status !== "lost") return;
      trying = true;
      try {
        const info = await invoke<RemoteInfo>("remote_reconnect");
        if (remote.status === "lost") {
          remote.info = info;
          lsp.reset();
          remote.status = "connected";
          checkDisk();
        }
      } catch {
        /* still unreachable; the next tick tries again */
      } finally {
        trying = false;
      }
    };
    const first = setTimeout(again, 1000);
    const timer = setInterval(again, 5000);
    return () => {
      clearTimeout(first);
      clearInterval(timer);
    };
  });

  /** How busy the remote is, as the backend reads it. Mirrors `Sample` in the core. */
  type SystemSample = {
    cpu: number;
    cores: number;
    /** The processor's, in degrees Celsius, where it has a sensor. */
    temperature: number | null;
    /** Over the last one, five and fifteen minutes. */
    load: [number, number, number];
    /** Kilobytes. */
    memUsed: number;
    memTotal: number;
    swapUsed: number;
    swapTotal: number;
    /** Bytes a second. */
    netDown: number;
    netUp: number;
    /** Kilobytes, of the disk the home folder is on. */
    diskUsed: number;
    diskTotal: number;
    /** Seconds. */
    uptime: number;
    /** Its memory is in mebibytes, and nought where the card does not say. */
    gpu: {
      usage: number;
      memUsed: number;
      memTotal: number;
      temperature: number | null;
      /** Watts. */
      power: number | null;
    } | null;
    /** Each core's share in use, in percent. */
    perCore: number[];
    /** Every process on one of its terminals: what the windows are running. */
    processes: Process[];
    /** Every tmux pane as it stands: what it runs, where, and when its window last printed. */
    panes: PaneNow[];
  };
  /** The latest reading; null until there is one, and on a machine there is none of. */
  let system = $state.raw<SystemSample | null>(null);

  /** The tmux window in front of someone's eyes, if one is. */
  const looking = () =>
    mode === "tmux" && document.hasFocus() ? (tmux.window?.id ?? null) : null;

  // Read from the first turn to tmux for as long as the remote is there,
  // whichever face is showing after that: it is a channel held open and a
  // few lines of shell on the remote, against several seconds of an empty
  // bar on every turn back, and a build that ends while the editor is
  // showing is still one to be told of.
  $effect(() => {
    if (!tmuxOpened || remote.status !== "connected") return;
    const samples = new Channel<SystemSample>();
    let watching = true;
    samples.onmessage = (sample) => {
      if (!watching) return;
      system = sample;
      const ended = watch.update(tmux.sessions, sample.processes, sample.panes, looking(), Date.now());
      // Something finished while this window was not the one in use: its
      // button in the taskbar says so, until the window is come back to.
      if (ended.length > 0 && !document.hasFocus()) {
        void appWindow.requestUserAttention(UserAttentionType.Informational).catch(() => {});
      }
    };
    invoke("sysinfo_start", { onSample: samples }).catch((e) => console.error("sysinfo_start failed", e));
    return () => {
      watching = false;
      system = null;
      watch.reset();
      void invoke("sysinfo_stop").catch(() => {});
    };
  });

  // Coming to a window is seeing what happened in it.
  $effect(() => {
    const window = tmux.window?.id;
    if (mode === "tmux" && window !== undefined) untrack(() => watch.look(window));
  });

  /** The window has the keyboard again: whatever it is showing has now been seen. */
  function onWindowFocus() {
    checkDisk();
    const window = looking();
    if (window !== null) watch.look(window);
  }

  /** A size given in mebibytes, in gigabytes to one place. */
  const gigabytes = (mebibytes: number) => (mebibytes / 1024).toFixed(1);

  /** Bytes a second, as short as it can be said: `340K`, `1.2M`. */
  function rate(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)}M`;
    return `${Math.round(bytes / 1024)}K`;
  }

  /** How long a machine has been up, in its two largest units: `3d 4h`. */
  function lasted(seconds: number): string {
    const minutes = Math.floor(seconds / 60);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);
    if (days > 0) return `${days}d ${hours % 24}h`;
    if (hours > 0) return `${hours}h ${minutes % 60}m`;
    return `${minutes}m`;
  }

  /** What the title bar says while tmux is showing: the session, and the window in front. */
  const tmuxTitle = $derived.by(() => {
    const session = tmux.session;
    if (!session) return null;
    const window = tmux.window;
    return window ? `${session.name} › ${window.name}` : session.name;
  });

  /** Close a pane, asking first if something is running in it. */
  async function closeTmuxPane(pane: TmuxPane) {
    if (busy(pane)) {
      const sure = await confirm({
        title: "Close pane",
        message: `${pane.command} is running in this pane.`,
        note: "Closing the pane ends it.",
        confirm: "Close Pane",
        danger: true,
      });
      if (!sure) return;
    }
    tmux.act({ do: "killPane", pane: pane.id });
  }


  /** Close a tmux window, of any session, asking first if that would end something. */
  async function closeTmuxWindow(sessionId: number, id: number) {
    const session = tmux.sessions.find((each) => each.id === sessionId);
    const window = session?.list.find((each) => each.id === id);
    if (!session || !window) return;
    const running = jobs(window.panes, system?.processes ?? []);
    const last = session.list.length === 1;
    if (running.length > 0 || last) {
      const what = [...new Set(running.map((job) => job.command.split(" ", 1)[0]))].join(", ");
      const sure = await confirm({
        title: "Close window",
        message:
          running.length > 0
            ? `${what} ${running.length === 1 ? "is" : "are"} running in "${window.name}".`
            : `Close "${window.name}"?`,
        note: last
          ? `It is the last window of "${session.name}", so the session ends with it.`
          : "Closing the window ends everything running in it.",
        confirm: "Close Window",
        danger: true,
      });
      if (!sure) return;
    }
    tmux.act({ do: "killWindow", window: id });
  }

  /** End a session. Always asked about: it is what was meant to last. */
  async function killTmuxSession(id: number) {
    const session = tmux.sessions.find((each) => each.id === id);
    if (!session) return;
    const sure = await confirm({
      title: "Kill session",
      message: `Kill the session "${session.name}"?`,
      note:
        `Everything running in its ${session.windows === 1 ? "window" : `${session.windows} windows`} ` +
        "is ended, and there is no getting it back.",
      confirm: "Kill Session",
      danger: true,
    });
    if (sure) tmux.act({ do: "killSession", session: id });
  }


  function onTmuxWindowContextMenu(event: MouseEvent, session: number, window: number) {
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        item("Rename", () => tmuxSidebar?.renameWindow(window)),
        SEP,
        item("Close Window", () => void closeTmuxWindow(session, window), true),
      ],
    };
  }

  /**
   * Something running in a window. A signal goes to it and to everything it
   * has started, whichever pane it is in and whether or not it is the one
   * that pane would hand a Ctrl+C to.
   */
  function onTmuxJobContextMenu(event: MouseEvent, job: Job) {
    const send = (signal: "INT" | "TERM" | "KILL") => () =>
      void invoke("remote_signal", { pid: job.pid, shell: job.shell, signal }).catch((e) =>
        message(String(e), { title: "tmux", kind: "error" }),
      );
    const name = job.command.split(" ", 1)[0];
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        item("Interrupt (Ctrl+C)", send("INT")),
        item("Terminate", send("TERM")),
        SEP,
        item(
          "Kill",
          () =>
            void confirm({
              title: "Kill process",
              message: `Kill ${name}?`,
              note: "It is ended at once, with no chance to finish what it is doing or clean up after itself.",
              confirm: "Kill",
              danger: true,
            }).then((sure) => sure && send("KILL")()),
          true,
        ),
      ],
    };
  }

  function onTmuxSessionContextMenu(event: MouseEvent, session: number) {
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        item("Rename", () => tmuxSidebar?.rename(session)),
        item("New Window", () => tmux.act({ do: "newWindow", session })),
        SEP,
        item("Kill Session", () => void killTmuxSession(session), true),
      ],
    };
  }

  /** A path or an address in a pane was Ctrl+clicked. A file opens in the editor. */
  function followTmuxLink(link: Link) {
    if (!("url" in link)) setMode("ide");
    followLink(link, null);
  }

  /** Something over the window has the keyboard, and a pane is not to take it back. */
  const overlaid = $derived.by(
    () =>
      paletteOpen ||
      appearanceOpen ||
      lspOpen ||
      connectOpen ||
      question !== null ||
      browse !== null ||
      ctx !== null ||
      docs.busy,
  );

  // --- the bottom bar ------------------------------------------------------------
  //
  // Its left half says where the window is working: on which machine, as
  // whom, and on what branch.

  /** This machine and whoever is at it, for while the window is working here. */
  let identity = $state.raw({ user: "", host: "" });

  /** Who and where the bar names: the remote's while on one, or being joined. */
  const where = $derived.by(() => {
    if (remote.remote && remote.info) return { user: remote.info.user, host: remote.info.host };
    if (remote.status === "connecting" && remote.pending) {
      const [user, host] = remote.pending.split("@");
      return { user, host: host ?? "" };
    }
    return identity;
  });

  /** A repository's state, as the backend reads it. For reading only. */
  type GitStatus = {
    branch: string;
    /** False when only the branch is known: there was no `git` to count with. */
    counted: boolean;
    staged: number;
    unstaged: number;
    untracked: number;
    conflicts: number;
    upstream: boolean;
    ahead: number;
    behind: number;
  };

  /** The folder the window is working in, which is whose repository is shown. */
  const workDir = $derived(workingDir());
  let git = $state.raw<GitStatus | null>(null);
  let gitAsking = false;

  /** Read it again. It moves with every save, and with what is done in a terminal. */
  async function refreshGit() {
    const dir = workDir;
    if (dir === null || remote.status === "connecting" || remote.status === "lost") {
      if (dir === null) git = null;
      return;
    }
    // One at a time: a slow answer is waited for, not asked for again.
    if (gitAsking) return;
    gitAsking = true;
    const status = await invoke<GitStatus | null>("git_status", { dir }).catch(() => null);
    gitAsking = false;
    // Still the folder that was asked about, and only if something moved.
    if (dir === workDir && JSON.stringify(status) !== JSON.stringify(git)) git = status;
  }

  // At once when the folder changes, or the machine it is on; then on a timer.
  $effect(() => {
    void workDir;
    void remote.status;
    untrack(() => void refreshGit());
  });

  // --- session ------------------------------------------------------------------
  //
  // What is open is written down as it changes and put back at the next
  // launch. Only the main window does either: a session is one window's worth
  // of state, and two windows taking turns to overwrite it would restore
  // whichever happened to write last.
  //
  // It holds what is open on this machine, what was open on each saved
  // connection, and which of those the window was on, so a window that was
  // working on a remote comes back to it.

  type Session = Workspace & {
    sidebarCollapsed: boolean;
    sidebarWidth: number;
    /** The saved connection the window is on, to come back to. */
    connection: string | null;
    remote: Record<string, Workspace>;
  };

  const keepsSession = appWindow.label === "main";
  /** Nothing is saved until the last session is back: half of it is not a session. */
  let sessionRestored = $state(false);

  /**
   * Go back to the saved connection the window was on when it closed. False
   * if that could not be done, for the caller to carry on here instead.
   */
  async function resume(id: string, local: Workspace): Promise<boolean> {
    if (!remote.known.some((connection) => connection.id === id)) return false;

    const result = await attempt(savedLabel(id), (trustNewKey) =>
      invoke<RemoteInfo>("remote_connect_saved", { id, trustNewKey }),
    );
    if (result !== null && "info" in result) {
      await goRemote(result.info, local);
      return true;
    }
    if (result !== null) {
      void message(`${result.error}\n\nCarrying on with what was open on this machine.`, {
        title: `Could not reconnect to ${savedLabel(id)}`,
        kind: "warning",
      });
    }
    return false;
  }

  async function restoreSession() {
    const session = await invoke<Session>("session_load");
    sidebarWidth = session.sidebarWidth;
    remoteStashes = session.remote;
    const local: Workspace = {
      folders: session.folders,
      unfolded: session.unfolded,
      files: session.files,
      active: session.active,
      layout: session.layout,
      workflows: session.workflows,
      workflow: session.workflow,
    };

    // A reloaded page starts over, but the connection it had is still up.
    const live = await invoke<RemoteInfo | null>("remote_state").catch(() => null);
    if (live) await goRemote(live, local);
    else if (session.connection === null || !(await resume(session.connection, local)))
      await restoreWorkspace(local);

    // Last, so the tree is already there when the sidebar opens onto it.
    sidebarCollapsed = session.sidebarCollapsed;
  }

  const session = $derived.by((): Session => {
    const here = captureWorkspace();
    // On a remote, "this machine" is what was put away on the way there.
    const id = remote.remote ? (remote.info?.saved ?? null) : null;
    return {
      ...(remote.remote ? (localStash ?? NOTHING) : here),
      sidebarCollapsed,
      sidebarWidth: Math.round(sidebarWidth),
      connection: id,
      remote: id === null ? remoteStashes : { ...remoteStashes, [id]: here },
    };
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

  // --- panes and tabs -----------------------------------------------------------
  //
  // The stage is one pane until it is split (`:sp`, `:vsp`, or the buttons at
  // the end of a strip), and each pane has a strip of its own for files and
  // terminals alike, in the order they were opened until one is dragged
  // somewhere else. `Layout` holds which tab is where; this is what the
  // gestures on them do.

  function tabOf(key: number): Tab | null {
    if (isTerminal(key)) {
      const terminal = terminals.find((t) => t.key === key);
      if (!terminal) return null;
      return {
        key,
        name: terminal.name,
        detail: terminal.cwd ?? "Terminal",
        icon: iconUrl("console"),
        dirty: false,
      };
    }
    const doc = docs.find(key);
    return (
      doc && {
        key,
        name: doc.name,
        detail: doc.path ?? "Not saved yet",
        icon: fileIcon(doc.name),
        dirty: doc.dirty,
      }
    );
  }

  /** A file is what the focused pane is showing, rather than a terminal or nothing. */
  const showingFile = $derived(docs.active !== null);

  /**
   * Panes that have let go of these files: the editor keeps each one's state
   * aside, for whichever pane takes the file up next.
   */
  function releaseFiles(pane: number, keys: number[]) {
    for (const key of keys) if (!isTerminal(key)) docs.editor.release(pane, key);
  }

  /**
   * Close a tab: the one showing in the focused pane unless told otherwise.
   * A file open in another pane as well only leaves this one. `force` is
   * `:q!`, which does not ask about unsaved changes.
   */
  function closeTab(key = layout.current.active, pane = layout.focused, force = false) {
    if (key === null) return;
    if (isTerminal(key)) closeTerminal(key);
    else if (layout.holding(key).length > 1) {
      docs.vimLines[pane]?.replaceChildren();
      releaseFiles(pane, [key]);
      layout.closeTab(pane, key);
    } else void closeFiles([key], { force });
  }

  /** Step to the next or previous tab of the focused pane, wrapping at the ends. */
  function cycleTabs(step: 1 | -1) {
    const { tabs, active, id } = layout.current;
    if (tabs.length < 2 || active === null) return;
    layout.open(tabs[(tabs.indexOf(active) + step + tabs.length) % tabs.length], id);
  }

  /** A tab was dropped: at `index` of the strip of pane `to`. */
  function moveTab(key: number, from: number, to: number, index: number) {
    if (from !== to) releaseFiles(from, [key]);
    layout.move(key, from, to, index);
  }

  /**
   * Another pane beside this one or below it. On the same file, as `:sp`
   * gives a second window on the buffer; a terminal gets a new terminal,
   * there being no second view of a shell to give.
   */
  function splitPane(dir: SplitDir, pane = layout.focused, key = layout.leaf(pane)?.active ?? null) {
    if (key === null) return;
    layout.split(dir, [isTerminal(key) ? makeTerminal() : key], pane);
  }

  /**
   * The buttons at the end of a strip: the tab in front moves to a new pane
   * beside this one or below it. With only the one tab there is nothing to
   * leave behind, and it is split as `:sp` would.
   */
  function splitOff(dir: SplitDir, pane: number) {
    const leaf = layout.leaf(pane);
    const key = leaf?.active ?? null;
    if (!leaf || key === null) return;
    if (leaf.tabs.length < 2) {
      splitPane(dir, pane, key);
      return;
    }
    const to = layout.split(dir, [], pane);
    moveTab(key, pane, to, -1);
  }

  /** `:sp file` and `:vsp file`. */
  async function splitOn(dir: SplitDir, file: string | null) {
    const pane = layout.focused;
    if (file === null) {
      splitPane(dir, pane);
      return;
    }
    const key = await docs.open(resolvePath(file), { show: false });
    if (key !== null) layout.split(dir, [key], pane);
  }

  /** Close a pane and keep what is in it: its tabs join the pane beside it. */
  function closePane(pane: number) {
    const leaf = layout.leaf(pane);
    if (!leaf || layout.leaves.length < 2) return;
    releaseFiles(pane, leaf.tabs);
    layout.closePane(pane);
  }

  /** `:only`. */
  function onlyPane() {
    for (const leaf of layout.leaves) {
      if (leaf.id !== layout.focused) releaseFiles(leaf.id, leaf.tabs);
    }
    layout.only();
  }

  /**
   * A path typed at vim's command line. A relative one is from the folder
   * the window is working in, as it would be from vim's working directory.
   */
  function resolvePath(file: string): string {
    if (/^([a-zA-Z]:[\\/]|[\\/])/.test(file)) return file;
    const base = workingDir();
    if (base === null) return file;
    const sep = base.includes("\\") ? "\\" : "/";
    return base.replace(/[\\/]$/, "") + sep + file;
  }

  const WINCMD: Record<string, Direction> = { h: "left", j: "down", k: "up", l: "right" };

  // Vim's commands that reach outside the buffer. `:q` closes the tab it is
  // typed in, and with it the pane if that was its last: which is what vim's
  // `:q` does to a window.
  docs.vimHooks = {
    write: () => void docs.save(),
    quit: (force) => closeTab(undefined, undefined, force),
    writeQuit: () =>
      void docs.save().then((saved) => {
        // A save that failed or was cancelled leaves the file open.
        if (saved) closeTab();
      }),
    cycle: cycleTabs,
    split: (dir, file) => void splitOn(dir, file),
    fresh: (dir) => void layout.split(dir, [docs.newFile({ show: false })]),
    edit: (file) => {
      // In the pane it was typed in, wherever else the file may be open.
      const pane = layout.focused;
      void docs.open(resolvePath(file), { show: false }).then((key) => {
        if (key !== null) layout.open(key, pane);
      });
    },
    close: () => closePane(layout.focused),
    only: onlyPane,
    wincmd: (arg) => {
      if (arg in WINCMD) layout.focusTowards(WINCMD[arg]);
      else if (arg === "w") layout.focusNext(1);
      else if (arg === "p" || arg === "W") layout.focusNext(-1);
      else if (arg === "s") splitPane("column");
      else if (arg === "v") splitPane("row");
      else if (arg === "c") closePane(layout.focused);
      else if (arg === "o") onlyPane();
      else if (arg === "q") closeTab();
    },
    // Every file, in every pane of this workflow. Terminals are left
    // running: a shell is not a buffer, and ending one is not something to
    // do in passing.
    quitAll: (force) => void closeFiles(flowFiles(), { force }),
    writeAll: () => void docs.saveAll(),
    writeQuitAll: () =>
      void docs.saveAll().then((saved) => {
        if (saved) void closeFiles(flowFiles());
      }),
    terminal: () => newTerminal(),
  };

  // --- the stage ------------------------------------------------------------------

  let stage = $state<HTMLElement | undefined>();

  /** Where a box of the layout goes, as CSS; `top` is how much of its top to leave. */
  function place(box: Rect, top = "0px"): string {
    return (
      `left:${box.x * 100}%;top:calc(${box.y * 100}% + ${top});` +
      `width:${box.w * 100}%;height:calc(${box.h * 100}% - ${top})`
    );
  }

  /** The line being dragged between two panes, while one is. */
  let dividing = $state<Divider | null>(null);

  function startDivide(event: PointerEvent, line: Divider) {
    if (event.button !== 0) return;
    dividing = line;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function divide(event: PointerEvent) {
    if (!dividing || !stage) return;
    const box = stage.getBoundingClientRect();
    const at =
      dividing.dir === "row"
        ? (event.clientX - box.left) / box.width
        : (event.clientY - box.top) / box.height;
    layout.resize(dividing.split, dividing.index, at);
  }

  /** The pane a dragged tab would join the end of, while it is over one's body. */
  const dropZone = $derived.by(() => {
    const { from, over } = tabDrag;
    if (!from || !over || over.gap !== null || over.pane === from.pane) return null;
    return layout.boxes.get(over.pane) ?? null;
  });

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

  // There are two hotkeys, Ctrl+Shift+P for the command list and Ctrl+Tab
  // to step through a pane's tabs: everything else is done from the list,
  // the menus and vim's own command line. The webview has a browser's worth of
  // its own (Ctrl+P prints, F5 reloads, Ctrl+F searches the page), and those
  // are stopped here. What is left alone is typing: the clipboard, undo,
  // moving by word, and whatever the editor or a shell makes of a key.
  const TYPING = new Set([
    "a", "c", "v", "x", "y", "z",
    "arrowleft", "arrowright", "arrowup", "arrowdown",
    "backspace", "delete", "home", "end", "insert",
    "control", "shift", "alt", "meta",
  ]);
  const HISTORY = new Set(["arrowleft", "arrowright", "home"]);

  /** Whether a key is one the webview would act on as a browser. */
  function isBrowserKey(event: KeyboardEvent): boolean {
    const key = event.key.toLowerCase();
    // Alt+F4 is the system's, and stays the way to close a window.
    if (/^f\d+$/.test(key)) return !(key === "f4" && event.altKey);
    // AltGr, which is how half the world types `@` and `{`.
    if (event.ctrlKey && event.altKey) return false;
    if (event.altKey) return HISTORY.has(key);
    return (event.ctrlKey || event.metaKey) && !TYPING.has(key);
  }

  // On the way down, for everything but the editor, vim's line and a terminal: those
  // are asked first, vim and a shell having uses of their own for Ctrl, and
  // are caught on the way back up with whatever they made nothing of.
  function onKeydownCapture(event: KeyboardEvent) {
    // From anywhere, a terminal included: no shell has a use for either.
    const key = event.key.toLowerCase();
    const palette = key === "p" && event.shiftKey;
    if (event.ctrlKey && !event.altKey && (palette || key === "tab")) {
      event.preventDefault();
      event.stopPropagation();
      // Not over a dialog: the window behind one is not taking commands.
      const blocked =
        docs.busy ||
        appearanceOpen ||
        lspOpen ||
        question !== null ||
        connectOpen ||
        browse !== null ||
        remote.status === "connecting";
      if (blocked) return;
      // Held down, Ctrl+Tab runs along the strip; the list opens the once.
      if (!palette) {
        if (paletteOpen) return;
        // The strip in sight: the editor's tabs, or the session's windows.
        if (mode === "tmux") tmux.cycle(event.shiftKey ? -1 : 1);
        else cycleTabs(event.shiftKey ? -1 : 1);
      } else if (!event.repeat) paletteOpen = true;
      return;
    }
    const typedInto =
      event.target instanceof Element && event.target.closest(".cm-editor, [data-vim-line], [data-terminal]") !== null;
    if (!typedInto && isBrowserKey(event)) event.preventDefault();
  }

  function onKeydown(event: KeyboardEvent) {
    if (isBrowserKey(event)) event.preventDefault();
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
   * The editor. On text it is the clipboard's menu; on the space past the
   * end of the file, which is nothing in particular, it is the window's.
   */
  function onEditorContextMenu(event: MouseEvent, pane: number) {
    const onText = event.target instanceof Element && event.target.closest(".cm-line") !== null;
    const edit = (command: EditCommand) => () => {
      // The menu took the focus to be clicked, and these act on what has it.
      docs.editor.focus(pane);
      void runEdit(command);
    };
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: onText
        ? [
            item("Cut", edit("cut")),
            item("Copy", edit("copy")),
            item("Paste", edit("paste")),
            SEP,
            item("Select All", edit("selectAll")),
          ]
        : [
            item("Paste", edit("paste")),
            item("Select All", edit("selectAll")),
            SEP,
            item("Refresh Page", refreshPage),
          ],
    };
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
        ? [item("Close Folder", () => void closeFolders([row.path]))]
        : [item("Rename", () => startRename(row)), item("Delete", () => void deleteRow(row), true)];
    // Only with a terminal in front: it is the one that is sent there.
    const terminal = activeTerminal;
    const jump =
      row?.folder && terminal !== null
        ? [item("Jump Terminal Here", () => jumpTerminal(terminal, row.path)), SEP]
        : [];
    // Reloading is for the empty space below the rows, not for a row.
    const items = [...jump, ...create, ...own, ...(row ? [] : [item("Refresh Page", refreshPage)])];
    // No line under the last entry.
    if (items.at(-1) === SEP) items.pop();

    ctx = { x: event.clientX, y: event.clientY, items };
  }


  /** A tab, in the strip of one pane. */
  function onTabContextMenu(event: MouseEvent, key: number, pane: number) {
    const own = isTerminal(key)
      ? [item("Kill Terminal", () => closeTerminal(key), true)]
      : [
          item("Save", () => void docs.save(key)),
          item("Save As…", () => void docs.saveAs(key)),
          item("Close", () => closeTab(key, pane)),
        ];
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        ...own,
        SEP,
        item("Split Right", () => splitPane("row", pane, key)),
        item("Split Down", () => splitPane("column", pane, key)),
      ],
    };
  }

  // Drives every surface in app.css at once, so opacity is one number in one
  // place rather than a rule per pane. Set on the root element because the
  // whole cascade reads it, including components this file never touches.
  $effect(() => {
    document.documentElement.style.setProperty("--bg-alpha", String(config.alpha));
  });

  // The theme, and then the accent: one colour in the config, and every
  // highlight in the window follows it.
  $effect(() => {
    const { syntax, mode, accent } = config.current.theme;
    applyTheme(syntax, mode);
    applyAccent(accent);
  });

  // The editor's share of the config: the font, the gutter, and each open
  // file's indentation. Re-run whenever the config is replaced.
  $effect(() => {
    const current = config.current;
    // Only the config is a dependency: `configure` walks the open files, and
    // must not re-run just because one was opened or closed.
    untrack(() => {
      docs.configure(current);
      configured++;
    });
  });
  /** Counts the configs taken on, for what reads from them by way of a plain function. */
  let configured = $state(0);

  // The language servers that are switched on. Which are installed is looked
  // up once the window is up, and again each time the dialog is opened.
  $effect(() => {
    const enabled = config.current.lsp.enabled;
    untrack(() => lsp.configure(enabled));
  });

  // Which way round the tree lists a folder is the config's to say.
  $effect(() => {
    const first = config.current.files.files_first;
    flows.filesFirst = first;
    for (const flow of flows.list) flow.tree.filesFirst = first;
  });

  // A folder opened or closed can change which project a file belongs to.
  $effect(() => {
    void tree.roots.length;
    untrack(() => lsp.sync());
  });

  // Which servers there are, and which shells, are facts about a machine:
  // asked when the window comes up, and again each time it lands on
  // another one.
  $effect(() => {
    if (remote.status === "local" || remote.status === "connected") {
      untrack(() => void lsp.refresh());
      void invoke<Shell[]>("terminal_shells").then(
        (found) => (shells = found),
        () => (shells = []),
      );
    }
  });

  // --- the command list ------------------------------------------------------------
  //
  // A few things to do by name, the one last done first. Made each time it
  // is looked at, so what is not possible just now is listed as such.

  const RECENT_KEY = "tide.commands.recent";

  /** The commands that have been run, by title, the latest first. */
  let recent = $state<string[]>(readRecent());

  function readRecent(): string[] {
    try {
      const kept: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
      return Array.isArray(kept) ? kept.filter((each) => typeof each === "string") : [];
    } catch {
      return [];
    }
  }

  function ranCommand(title: string) {
    recent = [title, ...recent.filter((each) => each !== title)];
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(recent));
    } catch {
      /* nowhere to keep it; the order lasts as long as the window does */
    }
  }

  const commands = $derived.by((): Command[] => {
    const all: Command[] = [
      { title: "New Terminal", disabled: mode !== "ide", run: () => newTerminal() },
      { title: "Toggle Terminal", disabled: mode !== "ide", run: toggleTerminal },
      // The window's two faces: the editor, and the remote's tmux sessions.
      mode === "ide"
        ? { title: "Switch to tmux", run: () => setMode("tmux") }
        : { title: "Switch to IDE", run: () => setMode("ide") },
      { title: "New Workflow", run: newFlow },
      { title: "Next Workflow", disabled: flows.list.length < 2, run: () => stepFlow(1) },
      { title: "Previous Workflow", disabled: flows.list.length < 2, run: () => stepFlow(-1) },
      {
        title: "Rename Workflow",
        run: () => {
          setMode("ide");
          // Once the list has gone, so the field it opens keeps the keyboard.
          requestAnimationFrame(() => workflowBar?.rename(flows.current.id));
        },
      },
      {
        title: "Close Workflow",
        disabled: flows.list.length < 2,
        run: () => void closeFlow(flows.current.id),
      },
      ...flows.list
        .filter((flow) => flow.id !== flows.current.id)
        .map((flow) => ({ title: `Workflow: ${flows.label(flow)}`, run: () => switchFlow(flow.id) })),
      { title: "Reconnect", disabled: remote.status !== "lost", run: () => void reconnect() },
      { title: "Reload Window", run: () => void refreshPage() },
      { title: "Close Window", run: closeWindow },
      // The same as Close Window, under the name some reach for.
      { title: "Exit", run: closeWindow },
    ];
    const used = (command: Command) => {
      const at = recent.indexOf(command.title);
      return at === -1 ? recent.length : at;
    };
    // Stable, so what has never been run stays in the order above.
    return all
      .sort((a, b) => used(a) - used(b))
      .map((command) => ({
        ...command,
        run: () => {
          ranCommand(command.title);
          command.run();
        },
      }));
  });

  /** A new config has arrived, from this window's dialog, another's, or the file. */
  function onConfigChanged(next: Config) {
    const exclude = JSON.stringify(config.current.files.exclude);
    config.current = next;
    // What the tree leaves out is decided where it is listed, so the listings
    // on screen are only as current as the last time they were read.
    if (JSON.stringify(next.files.exclude) !== exclude) void tree.refresh();
  }

  let diskTicks = 0;
  let gitTicks = 0;

  function checkDisk() {
    // Not while the window is between machines, when the paths that are open
    // and the machine that would be asked about them do not match; and not
    // of a connection that has dropped, which cannot answer.
    if (remote.status === "connecting" || remote.status === "lost") return;
    // Every stamp is a round trip on a remote, so it is asked half as often.
    if (remote.remote && diskTicks++ % 2 !== 0) return;
    void docs.checkDisk();
    void tree.poll();
    // That one runs `git`: once a second here, and every two on a remote.
    if (gitTicks++ % 4 === 0) void refreshGit();
  }

  onMount(() => {
    setEditor(docs.editor);

    void (async () => {
      await Promise.all([pins.refresh(), config.load(), remote.refreshKnown()]);
      void invoke<{ user: string; host: string }>("identity").then((who) => (identity = who));

      // Liveness beacon: proof in the backend's log that the frontend came up.
      void invoke("ui_ready", {
        detail: `APP_READY pins=${pins.list.length} scale=${config.appearance.scale}`,
      }).catch(() => {});

      // Not needed until a file is open, so not paid for before the window is up.
      loadIcons();

      // `tide.exe some-file`, `tide.exe .`, or "Open with" from Explorer.
      const startup = await invoke<{ path: string; dir: boolean }[]>("startup_files").catch(
        () => [],
      );
      for (const { path, dir } of startup) await (dir ? openFolder(path) : docs.open(path));

      // Asked to open something specific: do that, and leave the last session
      // where it is. Otherwise carry on from where things were left.
      if (keepsSession) {
        if (startup.length === 0) await restoreSession().catch((e) => console.error(e));
        sessionRestored = true;
        // The face it was showing, too: without asking who to connect to,
        // if the remote it was on is not there to go back to.
        if (startup.length === 0 && localStorage.getItem(MODE_KEY) === "tmux") setMode("tmux", false);
      } else {
        // A reloaded page starts over, but the connection it had is still up.
        const live = await invoke<RemoteInfo | null>("remote_state").catch(() => null);
        if (live) await goRemote(live);
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

    // The connection died under us. What is open stays open, unsaved edits
    // and all, to be saved once it is back.
    const stopRemoteLost = listen<RemoteInfo>("remote-lost", () => {
      if (remote.status === "connected") remote.status = "lost";
    });

    return () => {
      setEditor(null);
      clearInterval(diskPoll);
      void stopConfig.then((unlisten) => unlisten());
      void stopConfigError.then((unlisten) => unlisten());
      void stopRemoteLost.then((unlisten) => unlisten());
      void stopClose.then((unlisten) => unlisten());
    };
  });
</script>

<!-- Coming back to the window is when a stale file would be noticed, so that
     moment does not wait for the next tick of the poll. -->
<svelte:window
  onkeydowncapture={onKeydownCapture}
  onkeydown={onKeydown}
  onpointerdowncapture={onElsewhere}
  onfocusin={onElsewhere}
  oncontextmenu={onWindowContextMenu}
  onfocus={onWindowFocus}
/>

<div class="app">
  <TitleBar
    hasFile={mode === "ide" && showingFile}
    path={mode === "tmux"
      ? tmuxTitle
      : activeTerminal !== null
        ? (terminals.find((t) => t.key === activeTerminal)?.cwd ?? null)
        : docs.active
          ? (docs.active.path ?? docs.active.name)
          : null}
    folderCount={tree.roots.length}
    onnew={() => {
      // What the File and Terminal menus make is the editor's, and is made there.
      setMode("ide");
      docs.newFile();
    }}
    onopen={() => {
      setMode("ide");
      void docs.openDialog();
    }}
    onopenfolder={() => {
      setMode("ide");
      void openFolder();
    }}
    onclosefolder={() => void closeFolders(tree.roots.map((root) => root.path))}
    onsave={() => void docs.save()}
    onsaveas={() => void docs.saveAs()}
    onclosefile={() => closeTab()}
    sidebarCollapsed={mode === "tmux" ? tmuxSidebarCollapsed : sidebarCollapsed}
    ontogglesidebar={() => {
      if (mode === "tmux") tmuxSidebarCollapsed = !tmuxSidebarCollapsed;
      else sidebarCollapsed = !sidebarCollapsed;
    }}
    onnewterminal={() => {
      setMode("ide");
      newTerminal();
    }}
    shells={shells.map((shell) => shell.name)}
    onnewterminalin={(index) => {
      setMode("ide");
      newTerminal(shells[index]);
    }}
    onpalette={() => (paletteOpen = true)}
    remoteStatus={remote.status}
    known={remote.known}
    onconnect={openConnect}
    onconnectknown={(id) => void connectKnown(id)}
    onforgetknown={(id) => void forgetKnown(id)}
    ondisconnect={() => void disconnect()}
    onreconnect={() => void reconnect()}
    onnewwindow={newWindow}
    onnewworkflow={newFlow}
    onappearance={() => (appearanceOpen = true)}
    onlsp={() => (lspOpen = true)}
    onclosewindow={closeWindow}
    onquit={quit}
  />

  <!-- Out of sight while the window shows tmux, and otherwise untouched:
       every file, pane and terminal in it is as it was on coming back. -->
  <main class:resizing class:away={mode !== "ide"}>
    <Sidebar
      items={treeItems}
      activeKey={activeTreeKey}
      {selectedKey}
      width={sidebarCollapsed ? RAIL_WIDTH : sidebarWidth}
      collapsed={sidebarCollapsed}
      {resizing}
      onselect={onTreeSelect}
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
    <section class="stage" class:railed={sidebarCollapsed} bind:this={stage}>
      <!-- Laid out flat, each where the layout says it goes, rather than
           nested as the splits are: a pane then stays the same element
           however the splits around it change, and keeps its editor. -->
      {#each layout.leaves as leaf (leaf.id)}
        <Pane
          {leaf}
          style={place(layout.boxes.get(leaf.id)!)}
          focused={layout.focused === leaf.id}
          tabs={leaf.tabs.flatMap((key) => tabOf(key) ?? [])}
          closable={layout.leaves.length > 1}
          {docs}
          editor={config.current.editor}
          {configured}
          onfocus={() => layout.focus(leaf.id)}
          onselect={(key) => layout.open(key, leaf.id)}
          onclose={(key) => closeTab(key, leaf.id)}
          onmove={(key, to, index) => moveTab(key, leaf.id, to, index)}
          oncontext={(event, key) => onTabContextMenu(event, key, leaf.id)}
          onsplit={(dir) => splitOff(dir, leaf.id)}
          onlock={() => layout.toggleLock(leaf.id)}
          onclosepane={() => closePane(leaf.id)}
          oneditorcontext={(event) => onEditorContextMenu(event, leaf.id)}
        />
      {/each}

      <!-- Each for as long as its tab is open: with the last one goes
           xterm, and with each its shell. Beside the panes rather than
           inside them, and laid over the body of whichever pane has its
           tab, so one dragged to another pane is moved and not restarted. -->
      {#each terminals as terminal (terminal.key)}
        {@const leaf = layout.holding(terminal.key)[0]}
        {@const box = leaf && layout.boxes.get(leaf.id)}
        {@const front = leaf?.active === terminal.key}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="terminal"
          class:behind={!front}
          data-pane={leaf?.id}
          style={box ? place(box, "var(--tabs-height)") : "display:none"}
          onfocusin={() => leaf && layout.focus(leaf.id)}
          onpointerdowncapture={() => leaf && layout.focus(leaf.id)}
        >
          <TerminalView
            bind:this={terminalViews[terminal.key]}
            active={front}
            focused={front && layout.focused === leaf?.id}
            name={terminal.session}
            online={remote.status === "local" || remote.status === "connected"}
            cwd={terminal.cwd}
            shell={terminal.shell}
            keep={() => terminals.map((t) => t.session)}
            look={terminalLook}
            scrollback={config.current.terminal.scrollback}
            renderer={config.current.terminal.renderer}
            onexit={() => closeTerminal(terminal.key)}
            onopen={fromTerminal}
            onlink={(link) => followLink(link, terminal.cwd)}
          />
        </div>
      {/each}

      {#each layout.dividers as line (`${line.split}:${line.index}`)}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div
          class="divider"
          class:across={line.dir === "column"}
          class:dragging={dividing?.split === line.split && dividing.index === line.index}
          role="separator"
          aria-orientation={line.dir === "row" ? "vertical" : "horizontal"}
          style="left:{line.x * 100}%;top:{line.y * 100}%;--length:{line.length * 100}%"
          onpointerdown={(event) => startDivide(event, line)}
          onpointermove={divide}
          onpointerup={() => (dividing = null)}
          onpointercancel={() => (dividing = null)}
        ></div>
      {/each}

      {#if dropZone}
        <div class="drop-zone" style={place(dropZone)}></div>
      {/if}
    </section>
  </main>

  <!-- The other face: the remote's tmux. Not built until first turned to. -->
  {#if tmuxOpened}
    <main class:resizing class:away={mode !== "tmux"}>
      <TmuxSidebar
        bind:this={tmuxSidebar}
        sessions={tmux.sessions}
        activeId={tmux.state?.session ?? null}
        frontWindow={tmux.window?.id ?? null}
        processes={system?.processes ?? []}
        finished={watch.finished}
        active={watch.active}
        usable={remote.status === "connected" && tmux.status !== "missing"}
        attached={tmux.status === "attached"}
        width={tmuxSidebarCollapsed ? RAIL_WIDTH : sidebarWidth}
        collapsed={tmuxSidebarCollapsed}
        {resizing}
        onselect={(session) => tmux.select(session)}
        onwindow={(session, window) => tmux.show(window, session)}
        onjob={(session, window, pane) => {
          tmux.show(window, session);
          tmux.act({ do: "selectPane", pane });
        }}
        onnew={(name) => tmux.create(name)}
        onnewwindow={(session) => tmux.act({ do: "newWindow", session })}
        onrename={(session, name) => tmux.act({ do: "renameSession", session, name })}
        onrenamewindow={(window, name) => tmux.act({ do: "renameWindow", window, name })}
        onkill={(session) => void killTmuxSession(session)}
        onclosewindow={(session, window) => void closeTmuxWindow(session, window)}
        oncontext={onTmuxSessionContextMenu}
        onwindowcontext={onTmuxWindowContextMenu}
        onjobcontext={onTmuxJobContextMenu}
        onmove={(window, before, session) =>
          tmux.act({ do: "moveWindow", window, before: before ?? undefined, session })}
      />

      {#if !tmuxSidebarCollapsed}
        <SidebarResizer
          width={sidebarWidth}
          onresize={(w) => (sidebarWidth = w)}
          ondragging={(d) => (resizing = d)}
        />
      {/if}

      <TmuxView
        {tmux}
        shown={mode === "tmux"}
        blocked={overlaid}
        railed={tmuxSidebarCollapsed}
        remoteStatus={remote.status}
        host={where.host || "the remote"}
        live={system?.panes ?? []}
        home={remote.info?.home ?? ""}
        look={terminalLook}
        scrollback={config.current.terminal.scrollback}
        renderer={config.current.terminal.renderer}
        onconnect={openConnect}
        onlink={followTmuxLink}
        onclosepane={(pane) => void closeTmuxPane(pane)}
      />
    </main>
  {/if}

  {#if ctx}
    <ContextMenu x={ctx.x} y={ctx.y} items={ctx.items} onclose={() => (ctx = null)} />
  {/if}

  <PinBar
    pins={pins.list}
    onopen={openPin}
    onunpin={(pin) => void pins.remove(pin)}
    onmove={(pin, index) => void pins.move(pin, index)}
    wide={mode === "tmux"}
  >
    {#snippet start()}
      <!-- Where the window is working leads the bar, whichever face shows:
           it is one connection, and both are on it. -->
      <span
        class="where"
        data-status={remote.status}
        title={remote.status === "connecting"
          ? `Connecting to ${remote.pending ?? "the remote"}`
          : remote.status === "connected"
            ? `Remote: ${where.user}@${where.host}`
            : remote.status === "lost"
              ? `Disconnected from ${where.user}@${where.host}`
              : `Local: ${where.user}@${where.host}`}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path
            d="M3.6 1.2 L0.9 3.9 L3.6 6.6 M6.4 3.4 L9.1 6.1 L6.4 8.8"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        <span class="who">{where.user}{where.user && where.host ? "  " : ""}{where.host}</span>
      </span>
      <!-- While the window shows tmux, how hard the remote is working: a
           build is what runs there. Nothing until the first reading, and
           nothing at all of a machine that cannot be read. The bar is nearly
           all this group's then (`wide`), and what does not fit gives way
           from the least to the most worth knowing. -->
      {#if mode === "tmux"}
        {#if system}
          <span
            title="Processor in use, across {system.cores} cores.{system.perCore.length > 0
              ? ` Each: ${system.perCore.map((each) => Math.round(each)).join(' ')}`
              : ''}"
          >
            cpu {Math.round(system.cpu)}%{system.temperature !== null
              ? ` ${Math.round(system.temperature)}°C`
              : ""}
          </span>
          <span title="Memory in use, not counting what the kernel keeps as cache" data-drop="9">
            ram {gigabytes(system.memUsed / 1024)}/{gigabytes(system.memTotal / 1024)}G
          </span>
          {#if system.gpu}
            <span title="Graphics card: in use, its memory, its temperature and what it draws" data-drop="8">
              gpu {Math.round(system.gpu.usage)}%{system.gpu.memTotal > 0
                ? ` ${gigabytes(system.gpu.memUsed)}/${gigabytes(system.gpu.memTotal)}G`
                : ""}{system.gpu.temperature !== null
                ? ` ${Math.round(system.gpu.temperature)}°C`
                : ""}{system.gpu.power !== null ? ` ${Math.round(system.gpu.power)}W` : ""}
            </span>
          {/if}
          <span
            title="Load average over the last 1, 5 and 15 minutes, on {system.cores} cores"
            data-drop="7"
          >
            load {system.load.map((each) => each.toFixed(2)).join(" ")}
          </span>
          <span title="Network: arriving and leaving, a second" data-drop="6">
            net ↓{rate(system.netDown)} ↑{rate(system.netUp)}
          </span>
          {#if system.diskTotal > 0}
            <span title="The disk your home folder is on: used, of all of it" data-drop="5">
              disk {Math.round(system.diskUsed / 1024 / 1024)}/{Math.round(
                system.diskTotal / 1024 / 1024,
              )}G
            </span>
          {/if}
          {#if system.swapTotal > 0}
            <span title="Swap in use" data-drop="4">
              swap {gigabytes(system.swapUsed / 1024)}/{gigabytes(system.swapTotal / 1024)}G
            </span>
          {/if}
          <span title="How long the remote has been up" data-drop="3">
            up {lasted(system.uptime)}
          </span>
        {/if}
      {:else}
      <!-- Where the window is working, as one capsule: who, and on what.
           Filled while that is a remote, and in the colour of a fault once
           the connection to it has dropped. Then the repository the folder
           is in: its branch, and what in it is not yet where it is going.
           All of it is for reading; changing any of it is done elsewhere.

           `data-drop` is the order they give way in as the window narrows,
           lowest first (see `PinBar`), the group having half the bar. What
           is about the file being edited is not here: each pane says that
           at its own foot. -->
      <!-- Always all of it, at nought where there is nothing to count or
           no repository to count in, so the bar keeps its shape. -->
      <span class="git" title="Git branch" data-drop="6">
        {git?.branch ?? "no git"}
      </span>
      {#if git && git.conflicts > 0}
        <span class="fault" title="Files left in conflict by a merge" data-drop="5">
          {git.conflicts} conflicted
        </span>
      {/if}
      <span title="Files with changes that are not staged" data-drop="4">
        {git?.unstaged ?? 0} unstaged
      </span>
      <span
        title="Files with changes staged for the next commit"
        data-drop="3"
      >
        {git?.staged ?? 0} staged
      </span>
      <span title="Files git has not been told of" data-drop="1">
        {git?.untracked ?? 0} untracked
      </span>
      <span
        title={git?.counted && !git.upstream
          ? "This branch has not been pushed anywhere"
          : "Commits not yet pushed, and commits not yet pulled"}
        data-drop="2"
      >
        ↑{git?.ahead ?? 0} ↓{git?.behind ?? 0}
      </span>
      {/if}
    {/snippet}
    {#snippet info()}
      <!-- The workflows: which is in front, and the others to turn to. The
           editor's, so not while tmux is showing. -->
      {#if mode === "ide"}
        <WorkflowBar
          bind:this={workflowBar}
          flows={flows.list}
          current={flows.current.id}
          label={(flow) => flows.label(flow)}
          onselect={switchFlow}
          onnew={newFlow}
          onrename={(id, name) => flows.rename(id, name)}
          oncontext={onFlowContextMenu}
          onclose={(id) => void closeFlow(id)}
        />
      {/if}
      <!-- Which face the window shows: the editor, or the remote's tmux. -->
      <span class="modes" role="group" aria-label="What the window shows">
        <button
          class:on={mode === "ide"}
          aria-pressed={mode === "ide"}
          title="The editor, as it was left"
          onclick={() => setMode("ide")}
        >
          ide
        </button>
        <button
          class:on={mode === "tmux"}
          aria-pressed={mode === "tmux"}
          title="The remote's tmux sessions"
          onclick={() => setMode("tmux")}
        >
          tmux
        </button>
      </span>
    {/snippet}
  </PinBar>

  <CommandPalette
    open={paletteOpen}
    {commands}
    opacity={config.current.palette.background_opacity}
    acrylic={config.current.palette.material === "acrylic"}
    onclose={() => {
      paletteOpen = false;
      grabFocus();
    }}
  />

  <AppearanceDialog
    open={appearanceOpen}
    {config}
    onclose={() => (appearanceOpen = false)}
  />

  <LspDialog
    open={lspOpen}
    {lsp}
    {config}
    host={remote.remote ? (remote.info?.host ?? null) : null}
    onclose={() => (lspOpen = false)}
  />

  <ConnectDialog

    open={connectOpen}
    {keys}
    busy={remote.status === "connecting"}
    error={connectError}
    onsubmit={(request) => void connectNew(request)}
    oncancel={() => (connectOpen = false)}
  />

  <!-- A connection being made with no dialog of its own to wait in: a saved
       one, or one being made again. It holds the window still meanwhile. -->
  <Dialog
    open={remote.status === "connecting" && !connectOpen}
    title="Connecting"
    width={360}
    onclose={null}
  >
    <p class="dlg-empty">Connecting to {remote.pending}…</p>
  </Dialog>

  <RemoteBrowseDialog request={browse} />

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

  /* The face the window is not showing. Not there to be laid out, and all
     of it still alive: an editor keeps its place, and a terminal its shell. */
  main.away {
    display: none;
  }

  /* While dragging, stop the stage from selecting text under the cursor and
     keep the resize cursor even when the pointer strays off the handle. */
  main.resizing {
    cursor: col-resize;
    user-select: none;
  }

  /* What the app is showing, set apart from the chrome around it by being a
     shade darker and nothing else: no line is drawn round it.

     It is also the painted surface for the tabs and the editor, which draw no
     background of their own: one layer here is what keeps the window a single
     opacity.

     The panes, their terminals and the lines between them are all placed
     in it absolutely, so it is the positioning context for them. */
  .stage {
    --tabs-height: 26px;

    position: relative;
    flex: 1;

    min-width: 0;
    min-height: 0;
    margin: var(--viewport-inset) var(--viewport-inset) var(--viewport-inset) 0;
    background: var(--bg-viewport-wash);
    border-radius: var(--viewport-radius);
    transition: margin-left 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
    /* Keeps the content inside the rounded corners. Safe here, unlike on the
       sidebar: nothing in the stage needs to escape it. */
    overflow: hidden;
  }

  .stage.railed {
    margin-left: var(--viewport-inset);
  }

  /* A terminal, over the body of the pane its tab is in: everything of the
     pane but its strip. */
  .terminal {
    position: absolute;
  }
  /* With another tab in front it is still laid over the pane, and though
     nothing of it shows it would take every click meant for the editor
     under it, and the keyboard with them. */
  .terminal.behind {
    pointer-events: none;
  }

  /* The line between two panes, and the handle that moves it. The handle is
     wide enough to hit; what shows is a hairline down its middle. */
  .divider {
    position: absolute;
    z-index: 5;
    width: 7px;
    height: var(--length);
    transform: translateX(-50%);
    cursor: col-resize;
  }
  .divider.across {
    width: var(--length);
    height: 7px;
    transform: translateY(-50%);
    cursor: row-resize;
  }
  .divider::after {
    content: "";
    position: absolute;
    inset: 0 3px;
    background: var(--border);
    transition: background 120ms ease;
  }
  .divider.across::after {
    inset: 3px 0;
  }
  .divider:hover::after,
  .divider.dragging::after {
    background: var(--accent);
  }

  /* The pane a dragged tab would join. Not there to be clicked: the pane
     under it is still what the pointer is over. */
  .drop-zone {
    position: absolute;
    z-index: 6;
    background: var(--accent-soft);
    outline: 1px solid var(--accent);
    outline-offset: -1px;
    pointer-events: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .stage {
      transition: none;
    }
  }

  /* The left end of the bottom bar. Where the window is working is a
     capsule: tinted on this machine and while a remote is being joined,
     filled on a remote, and in the colour of a fault once the connection
     has dropped. Nothing in the bar is faded: it is small enough as it is. The rest is the bar's own
     text, in the accent like the rest of it. */
  .where {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    height: 17px;
    max-width: 16rem;
    padding: 0 0.6rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  .where[data-status="connected"] {
    background: var(--accent);
    color: var(--accent-ink);
  }
  .where[data-status="lost"] {
    background: var(--danger);
    color: var(--accent-ink);
  }
  .where svg {
    flex: none;
  }
  /* A machine or a branch with a very long name is cut short rather than
     allowed to push everything else out of the bar. */
  .who,
  .git {
    max-width: 14rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fault {
    color: var(--danger);
  }

  /* The right end of the bottom bar: which face the window shows. One
     capsule, as where it is working is at the other end, with the half that
     is showing filled. */
  .modes {
    display: inline-flex;
    height: 17px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    overflow: hidden;
  }
  .modes button {
    padding: 0 0.65rem;
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    font: inherit;
    line-height: 17px;
  }
  .modes button:hover {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .modes button.on {
    background: var(--accent);
    color: var(--accent-ink);
  }
</style>
