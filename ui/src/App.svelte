<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { message } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import AppearanceDialog from "./lib/AppearanceDialog.svelte";
  import ChangedDialog from "./lib/ChangedDialog.svelte";
  import ConfirmDialog, { type Confirmation } from "./lib/ConfirmDialog.svelte";
  import ConnectDialog from "./lib/ConnectDialog.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import Dialog from "./lib/Dialog.svelte";
  import Editor from "./lib/Editor.svelte";
  import PinBar from "./lib/PinBar.svelte";
  import RemoteBrowseDialog, { type BrowseRequest } from "./lib/RemoteBrowseDialog.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SidebarResizer from "./lib/SidebarResizer.svelte";
  import TabBar, { type Tab } from "./lib/TabBar.svelte";
  import TerminalView from "./lib/TerminalView.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import UnsavedDialog from "./lib/UnsavedDialog.svelte";

  import { applyAccent } from "./lib/accent";
  import { item, SEP, type ContextMenuState } from "./lib/contextMenu";
  import { setEditor } from "./lib/edit";
  import { iconUrl } from "./lib/icons";
  import { indentLabel } from "./lib/indent";
  import { languageOf } from "./lib/languages";
  import { RAIL_WIDTH } from "./lib/layout";
  import { ConfigStore, type Config } from "./lib/state/config.svelte";
  import { Documents } from "./lib/state/documents.svelte";
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
  import { Tree } from "./lib/state/tree.svelte";
  import type { Pin, SidebarItem } from "./lib/types";

  const pins = new Pins();
  const config = new ConfigStore();
  const docs = new Documents();
  const tree = new Tree();
  const remote = new RemoteStore();

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

  // --- terminals ----------------------------------------------------------------
  //
  // A terminal is a tab like a file is, and there can be several. Each is a
  // shell for whatever this window is working on. None exists until one is
  // asked for, and closing its tab ends it: there is no hidden state in
  // between, so a window with no terminal tab is running no shell.

  type TerminalTab = {
    key: number;
    name: string;
    /** Where the shell was started, fixed for as long as it lives. */
    cwd: string | null;
  };

  let terminals = $state<TerminalTab[]>([]);
  /** The terminal being shown, or null while a file is. */
  let activeTerminal = $state<number | null>(null);
  /** The one last shown, for Ctrl+` to come back to. */
  let lastTerminal: number | null = null;
  /** Each open terminal's view, by key, for the menus to act on. */
  const terminalViews = $state<Record<number, TerminalView | undefined>>({});

  // Files are keyed upwards from 1 and terminals downwards from -1, so the
  // tab strip can hold both under one kind of key and tell them apart.
  let nextTerminalKey = -1;
  let nextTerminalNumber = 1;
  const isTerminal = (key: number) => key < 0;

  // The terminal is set in the editor's font, the two being read side by
  // side, at a size and with a cursor of its own.
  const terminalLook = $derived({
    fontFamily: config.current.editor.font_family,
    fontSize: config.current.terminal.font_size,
    cursor: config.current.terminal.cursor,
  });

  /**
   * Where a new shell starts: the open folder, or with none open, the folder
   * of the file being edited. With several folders open it is the one that
   * file is in, and the first of them if it is in none. Null leaves it to the
   * backend, which uses the home folder.
   */
  function terminalDir(): string | null {
    const file = docs.active?.path ?? null;
    const roots = tree.roots.map((root) => root.path);
    // The deepest: with a project and one of its subfolders both open, the
    // file belongs to the more specific one.
    const holding = roots
      .filter((root) => file !== null && segmentsBelow(root, file) !== null)
      .sort((a, b) => b.length - a.length)[0];
    return holding ?? roots[0] ?? (file ? dirName(file) : null);
  }

  /** Terminal → New Terminal. */
  function newTerminal() {
    // Numbered from the top again once the last one has gone.
    if (terminals.length === 0) nextTerminalNumber = 1;
    const number = nextTerminalNumber++;
    const key = nextTerminalKey--;
    terminals.push({
      key,
      name: number === 1 ? "Terminal" : `Terminal ${number}`,
      cwd: terminalDir(),
    });
    activeTerminal = key;
  }

  /** A file has come to the front; whichever terminal was showing gives way. */
  function leaveTerminal() {
    if (activeTerminal === null) return;
    lastTerminal = activeTerminal;
    activeTerminal = null;
  }
  docs.onselect = leaveTerminal;

  /** Ctrl+`: to the terminal and back, opening one if there is none. */
  function toggleTerminal() {
    if (activeTerminal !== null) {
      leaveTerminal();
      docs.editor.focus();
    } else if (terminals.length > 0) {
      const back = terminals.find((t) => t.key === lastTerminal) ?? terminals.at(-1)!;
      activeTerminal = back.key;
    } else {
      newTerminal();
    }
  }

  /**
   * Close a terminal's tab, which ends its shell and whatever is running in
   * it. Also where a shell that exits by itself lands.
   */
  function closeTerminal(key: number) {
    const order = tabs.map((tab) => tab.key);
    const index = order.indexOf(key);
    terminals = terminals.filter((t) => t.key !== key);
    delete terminalViews[key];
    if (activeTerminal !== key) return;

    // The neighbour that slides into its place, else the one before it.
    activeTerminal = null;
    const next = order[index + 1] ?? order[index - 1];
    if (next !== undefined) selectTab(next);
  }

  // The last file closed, however it was closed, and there is a terminal
  // open: that is what there is to show, so show it rather than a blank
  // editor with a tab nobody is on.
  $effect(() => {
    if (docs.activeKey !== null || activeTerminal !== null || terminals.length === 0) return;
    const back = terminals.find((t) => t.key === lastTerminal) ?? terminals.at(-1)!;
    activeTerminal = back.key;
  });

  function inTerminal(target: EventTarget | null): boolean {
    return target instanceof Element && target.closest("[data-terminal]") !== null;
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
  };
  const NOTHING: Workspace = { folders: [], unfolded: [], files: [], active: null };

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

  function captureWorkspace(): Workspace {
    return {
      folders: tree.roots.map((root) => root.path),
      unfolded: tree.unfolded,
      files: docs.list.flatMap((doc) => (doc.path === null ? [] : [doc.path])),
      active: docs.active?.path ?? null,
    };
  }

  /** Open what a workspace had open. What has since gone is left out quietly. */
  async function restoreWorkspace(workspace: Workspace) {
    if (workspace.folders.length > 0) loadFolderIcons();
    await tree.restore(workspace.folders, workspace.unfolded);
    for (const path of workspace.files) await docs.open(path, { quiet: true });

    const active = docs.list.find((d) => d.path !== null && d.path === workspace.active);
    if (active) docs.select(active.key);
  }

  /** Close everything, without asking: whoever calls this already has. */
  function clearWorkspace() {
    // Unmounting a terminal is what ends its shell.
    for (const terminal of terminals) delete terminalViews[terminal.key];
    terminals = [];
    activeTerminal = null;
    lastTerminal = null;
    docs.clear();
    tree.clear();
    selectedKey = null;
    naming = null;
  }

  function browseRemote(mode: BrowseRequest["mode"], suggested = ""): Promise<string | null> {
    // A file that already lives somewhere is saved from there; anything else
    // starts where the last choice was made, or in the folder that is open.
    const start = suggested.includes("/") ? dirName(suggested) : (lastBrowsed ?? terminalDir());
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
              "and Thread will refuse to connect if it ever changes.",
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
      message: `Keep ${info.label}, so Thread can connect to it again without asking.`,
      note:
        "A saved connection is listed under Remote → Connect Known, and Thread reopens it, " +
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
      void message(String(e), { title: "Thread", kind: "error" });
    }
  }

  const savedLabel = (id: string) => {
    const saved = remote.known.find((connection) => connection.id === id);
    return saved ? connectionLabel(saved) : "the remote";
  };

  /** Remote → Connect Known. */
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
    remote.status = "connected";
    checkDisk();
  }

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

  /** The folder the window is working in, which is whose branch is shown. */
  const workDir = $derived(terminalDir());
  let gitBranch = $state<string | null>(null);

  /** Read the branch again. It moves when something is checked out elsewhere. */
  async function refreshGit() {
    const dir = workDir;
    if (dir === null || remote.status === "connecting" || remote.status === "lost") {
      if (dir === null) gitBranch = null;
      return;
    }
    const branch = await invoke<string | null>("git_branch", { dir }).catch(() => null);
    // Still the folder that was asked about.
    if (dir === workDir) gitBranch = branch;
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

  // --- tabs -------------------------------------------------------------------
  //
  // One strip for files and terminals, in the order they were opened.

  /** When each tab was first seen, which is where in the strip it sits. */
  const tabSeen = new Map<number, number>();

  const tabs = $derived.by(() => {
    const all = [
      ...docs.list.map(
        (doc): Tab => ({
          key: doc.key,
          name: doc.name,
          detail: doc.path ?? "Not saved yet",
          icon: fileIcon(doc.name),
          dirty: doc.dirty,
        }),
      ),
      ...terminals.map(
        (terminal): Tab => ({
          key: terminal.key,
          name: terminal.name,
          detail: terminal.cwd ?? "Terminal",
          icon: iconUrl("console"),
          dirty: false,
        }),
      ),
    ];
    for (const tab of all) if (!tabSeen.has(tab.key)) tabSeen.set(tab.key, tabSeen.size);
    return all.sort((a, b) => tabSeen.get(a.key)! - tabSeen.get(b.key)!);
  });

  const activeTab = $derived(activeTerminal ?? docs.activeKey);
  /** A file is what is on screen, rather than a terminal in front of one. */
  const showingFile = $derived(docs.active !== null && activeTerminal === null);

  function selectTab(key: number) {
    if (isTerminal(key)) {
      activeTerminal = key;
      return;
    }
    const fromTerminal = activeTerminal !== null;
    docs.select(key);
    // Coming from a terminal the keyboard is still in it, behind the file.
    if (fromTerminal) docs.editor.focus();
  }

  /** Close a tab, the one showing unless told otherwise. */
  function closeTab(key: number | null = activeTab) {
    if (key === null) return;
    if (isTerminal(key)) closeTerminal(key);
    else void docs.close(key);
  }

  /** Step to the next or previous tab, wrapping at the ends. */
  function cycleTabs(step: 1 | -1) {
    if (tabs.length < 2) return;
    const index = tabs.findIndex((tab) => tab.key === activeTab);
    selectTab(tabs[(index + step + tabs.length) % tabs.length].key);
  }

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
    // A dialog is up; the window behind it is not taking commands.
    const blocked =
      docs.busy ||
      appearanceOpen ||
      question !== null ||
      connectOpen ||
      browse !== null ||
      remote.status === "connecting";

    // Ctrl+` goes to the terminal and back, and with Shift opens another;
    // from anywhere, a terminal included.
    if (event.code === "Backquote") {
      event.preventDefault();
      event.stopPropagation();
      if (blocked || event.repeat) return;
      if (event.shiftKey) newTerminal();
      else toggleTerminal();
      return;
    }

    const key = event.key.toLowerCase();
    // Typed into a terminal, the rest belong to the shell: Ctrl+W there
    // deletes a word, and must not close the tab. Ctrl+Tab is the exception,
    // being the way out to the other tabs.
    if (inTerminal(event.target) && key !== "tab") return;
    if (!SHORTCUTS.has(key)) return;
    event.preventDefault();
    event.stopPropagation();

    if (blocked) return;

    if (key === "tab") {
      cycleTabs(event.shiftKey ? -1 : 1);
      return;
    }
    // Holding the others down should not open a stack of dialogs or files.
    if (event.repeat) return;

    if (key === "n") docs.newFile();
    else if (key === "o" && event.shiftKey) void openFolder();
    else if (key === "o") void docs.openDialog();
    else if (key === "w") closeTab();
    // There is a file behind a terminal's tab, but it is not what is on screen.
    else if (activeTerminal !== null) return;
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

  /** A terminal. The way to copy out of it, and one of the ways to end it. */
  function onTerminalContextMenu(event: MouseEvent, key: number) {
    const view = terminalViews[key];
    const back = (act: () => void) => () => {
      act();
      // The menu took the focus to be clicked; typing carries on after it.
      view?.focus();
    };
    // Only with something selected: there is nothing else it could copy.
    const copy = view?.hasSelection() ? [item("Copy", back(() => view.copy()))] : [];

    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        ...copy,
        item("Paste", back(() => view?.paste())),
        SEP,
        item("Kill Terminal", () => closeTerminal(key), true),
      ],
    };
  }

  /** A tab. */
  function onTabContextMenu(event: MouseEvent, key: number) {
    const own = isTerminal(key)
      ? [item("Kill Terminal", () => closeTerminal(key), true)]
      : [
          item("Save", () => void docs.save(key)),
          item("Save As…", () => void docs.saveAs(key)),
          item("Close", () => void docs.close(key)),
        ];
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [...own, SEP, item("Refresh Page", refreshPage)],
    };
  }

  // Drives every surface in app.css at once, so opacity is one number in one
  // place rather than a rule per pane. Set on the root element because the
  // whole cascade reads it, including components this file never touches.
  $effect(() => {
    document.documentElement.style.setProperty("--bg-alpha", String(config.alpha));
  });

  // The accent, likewise: one colour in the config, and every highlight in
  // the window follows it.
  $effect(() => {
    applyAccent(config.current.theme.accent);
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

  let diskTicks = 0;
  let gitTicks = 0;

  function checkDisk() {
    // Not while the window is between machines, when the paths that are open
    // and the machine that would be asked about them do not match; and not
    // of a connection that has dropped, which cannot answer.
    if (remote.status === "connecting" || remote.status === "lost") return;
    // Every stamp is a round trip on a remote, so it is asked a third as often.
    if (remote.remote && diskTicks++ % 3 !== 0) return;
    void docs.checkDisk();
    void tree.poll();
    // A checkout is rarer than a save; every third of these is often enough.
    if (gitTicks++ % 3 === 0) void refreshGit();
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
  onkeydowncapture={onKeydown}
  oncontextmenu={onWindowContextMenu}
  onfocus={checkDisk}
/>

<div class="app">
  <TitleBar
    hasFile={showingFile}
    path={activeTerminal !== null
      ? (terminals.find((t) => t.key === activeTerminal)?.cwd ?? null)
      : docs.active
        ? (docs.active.path ?? docs.active.name)
        : null}
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
    onnewterminal={newTerminal}
    remoteStatus={remote.status}
    known={remote.known}
    onconnect={openConnect}
    onconnectknown={(id) => void connectKnown(id)}
    onforgetknown={(id) => void forgetKnown(id)}
    ondisconnect={() => void disconnect()}
    onreconnect={() => void reconnect()}
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
          activeKey={activeTab}
          onselect={selectTab}
          onclose={closeTab}
          oncontext={onTabContextMenu}
        />
      {/if}

      <div class="editor-area">
        <!-- Hidden, not removed, behind a terminal: the editor keeps its
             scroll position and its measurements. -->
        <div class="pane editing" class:hidden={activeTerminal !== null}>
          <div class="text">
            <Editor host={docs.editor} />
          </div>
          <!-- Vim's `:` line, `/` search and messages are put here by
               `vim.ts`: one line of the editor, in the editor's own font and
               at its line height, that takes the place of the last line on
               screen for as long as it has something in it. Always present,
               so there is somewhere to put them the moment vim asks. -->
          <div
            class="vim-line"
            data-vim-line
            bind:this={docs.vimLine}
            style:font-family={config.current.editor.font_family}
            style:font-size="{config.current.editor.font_size}px"
            style:height="{config.current.editor.font_size * config.current.editor.line_height}px"
          ></div>
        </div>

        <!-- Each for as long as its tab is open: with the last one goes
             xterm, and with each its shell. -->
        {#each terminals as terminal (terminal.key)}
          <TerminalView
            bind:this={terminalViews[terminal.key]}
            active={activeTerminal === terminal.key}
            cwd={terminal.cwd}
            look={terminalLook}
            scrollback={config.current.terminal.scrollback}
            onexit={() => closeTerminal(terminal.key)}
            oncontext={(event) => onTerminalContextMenu(event, terminal.key)}
          />
        {/each}
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
      <!-- Where the window is working: this machine or a remote, then as
           whom, on what, in which mode, and on which branch. Each its own
           colour, so the eye finds one without reading the rest. All of it is
           for reading; changing any of it is done from the menus.

           `data-drop` is the order they give way in as the window narrows,
           lowest first (see `PinBar`). The mode has none: it is what the
           next key depends on, and is the last thing to go. -->
      <span class="place" data-status={remote.status} data-drop="3">
        {#if remote.status === "connecting"}
          Connecting
        {:else if remote.status === "connected"}
          Remote
        {:else if remote.status === "lost"}
          Disconnected
        {:else}
          Local
        {/if}
      </span>
      {#if where.user}
        <span class="user" data-drop="1">{where.user}</span>
      {/if}
      {#if where.host}
        <span class="host" data-drop="2">{where.host}</span>
      {/if}
      {#if docs.vimMode && showingFile}
        <span class="mode" data-mode={docs.vimMode}>{docs.vimMode}</span>
      {/if}
      {#if gitBranch}
        <span class="git" title="Git branch" data-drop="4">{gitBranch}</span>
      {/if}
    {/snippet}
    {#snippet info()}
      <!-- What rarely changes goes first as the window narrows; where the
           cursor is stays to the end. -->
      {#if docs.active && showingFile}
        <span>Ln {docs.cursor.line}, Col {docs.cursor.col}</span>
        <span data-drop="3">{indentLabel(docs.active.indent)}</span>
        <span data-drop="2">{docs.active.eol === "crlf" ? "CRLF" : "LF"}</span>
        <span data-drop="1">{docs.active.bom ? "UTF-8 with BOM" : "UTF-8"}</span>
        <span data-drop="4">{languageOf(docs.active.name)}</span>
      {/if}
    {/snippet}
  </PinBar>

  <AppearanceDialog
    open={appearanceOpen}
    {config}
    onclose={() => (appearanceOpen = false)}
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
     opacity. */
  .stage {
    display: flex;
    flex-direction: column;
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

  /* Whatever the tab strip leaves. The editor and the terminals fill it
     absolutely, so it is the positioning context for them. */
  .editor-area {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  .pane {
    position: absolute;
    inset: 0;
  }
  .pane.hidden {
    visibility: hidden;
  }
  /* The editor's pane is the text, and under it vim's line when it has
     something to say: the text gives up that much height and gets it back. */
  .pane.editing {
    display: flex;
    flex-direction: column;
  }
  .text {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .stage {
      transition: none;
    }
  }

  /* Vim's command line: a line across the foot of the editor, as vim has it.
     It is set as a line of the file is (the font and the height come from the
     config, inline), and the text above gives up exactly that much, so it
     reads as the last line on screen having been swapped for it. Nothing is
     drawn for it, no surface and no rule. Not there at all while it is empty.
     What goes in it is built by the vim extension, not by this component, so
     it is reached with `:global`. */
  .vim-line {
    flex: none;
    box-sizing: content-box;
    display: flex;
    align-items: center;
    /* In line with the line numbers above it, which also clears the card's
       rounded corner. */
    padding: 0 14px;
    color: var(--fg);
  }
  .vim-line:empty {
    display: none;
  }
  /* The extension's panel fills the bar, so the field in it has the width. */
  .vim-line :global(> *) {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
  }
  /* Everything the extension puts in here takes the window's own font and
     colour, over the monospace and the hard red it asks for inline. */
  .vim-line :global(*) {
    color: inherit !important;
    font-family: inherit !important;
    font-size: inherit !important;
  }
  .vim-line :global(input) {
    flex: 1;
    min-width: 0;
    padding: 0;
    background: transparent;
    border: none;
    outline: none;
  }
  .vim-line :global(.cm-vim-message) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The left end of the bottom bar. Every item is the bar's own text, with
     nothing drawn round it; the colour is what tells one from the next. */
  .place {
    color: #94e2d5;
  }
  .place[data-status="connected"] {
    color: var(--ok);
  }
  .place[data-status="connecting"] {
    color: #f9e2af;
  }
  .place[data-status="lost"] {
    color: var(--danger);
  }

  .user {
    color: #fab387;
  }
  .host {
    color: #89b4fa;
  }
  /* Named the way the status items are: "Normal", not vim's shouted "NORMAL". */
  .mode {
    color: var(--accent);
    text-transform: capitalize;
  }
  .git {
    color: #f5c2e7;
  }
  /* A machine or a branch with a very long name is cut short rather than
     allowed to push everything else out of the bar. */
  .host,
  .git {
    max-width: 14rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
