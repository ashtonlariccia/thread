<script lang="ts">
  /**
   * Choosing a file or a folder on a remote.
   *
   * The system's own dialogs can only see this machine, so on a remote this
   * stands in for all three of them: open a file, open a folder, save as. It
   * is a path you can type and a listing you can walk, and nothing more.
   */
  import { invoke } from "@tauri-apps/api/core";

  import Dialog from "./Dialog.svelte";
  import { fileIcon, folderIcon } from "./state/icons.svelte";
  import type { Entry } from "./state/tree.svelte";

  export type BrowseRequest = {
    mode: "file" | "folder" | "save";
    /** The folder to start in; null is the remote's home. */
    start: string | null;
    /** For a save: the name to start with. */
    name?: string;
    /** Called once, with the path chosen or null if none was. */
    resolve: (path: string | null) => void;
  };

  type Props = {
    request: BrowseRequest | null;
  };

  let { request }: Props = $props();

  const TITLES = { file: "Open File", folder: "Open Folder", save: "Save As" };

  /** The folder being shown, as the server spells it. */
  let path = $state("");
  /** What is in the path box: `path`, until it is typed over. */
  let typed = $state("");
  let entries = $state.raw<Entry[]>([]);
  let name = $state("");
  let loading = $state(false);
  let error = $state<string | null>(null);

  /** Folders only, when a folder is what is being chosen. */
  const shown = $derived(
    request?.mode === "folder" ? entries.filter((entry) => entry.dir) : entries,
  );

  // A new request starts over: where it says to, with nothing left from the last.
  $effect(() => {
    if (!request) return;
    name = request.name ?? "";
    error = null;
    entries = [];
    void go(request.start);
  });

  let latest = 0;

  /** Show a folder. A path that cannot be listed leaves the one on screen. */
  async function go(target: string | null) {
    const asked = ++latest;
    loading = true;
    try {
      const listing = await invoke<{ path: string; entries: Entry[] }>("remote_browse", {
        path: target,
      });
      // A slower answer to an earlier question must not replace a later one.
      if (asked !== latest) return;
      path = listing.path;
      typed = listing.path;
      entries = listing.entries;
      error = null;
    } catch (e) {
      if (asked === latest) error = String(e);
    } finally {
      if (asked === latest) loading = false;
    }
  }

  const join = (dir: string, child: string) => `${dir.replace(/\/+$/, "")}/${child}`;

  function finish(chosen: string | null) {
    const asked = request;
    asked?.resolve(chosen);
  }

  function choose(entry: Entry) {
    if (entry.dir) void go(entry.path);
    else if (request?.mode === "file") finish(entry.path);
    // Saving over a file: take its name, and leave the deciding to Save.
    else if (request?.mode === "save") name = entry.name;
  }

  function confirm() {
    if (!request || path === "") return;
    if (request.mode === "folder") finish(path);
    else if (request.mode === "save" && name.trim() !== "") finish(join(path, name.trim()));
  }

  function onPathKey(event: KeyboardEvent) {
    if (event.key !== "Enter") return;
    event.preventDefault();
    void go(typed.trim() || null);
  }
</script>

<Dialog
  open={request !== null}
  title={request ? TITLES[request.mode] : ""}
  width={520}
  scrolls
  onclose={() => finish(null)}
>
  {#if request}
    <div class="bar">
      <button
        class="up"
        title="Up a folder"
        aria-label="Up a folder"
        disabled={path === "/" || path === ""}
        onclick={() => void go(join(path, ".."))}
      >
        <svg width="12" height="12" viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M8 13 V3.5 M3.8 7.6 L8 3.4 L12.2 7.6"
            fill="none"
            stroke="currentColor"
            stroke-width="1.4"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
      <input
        class="path"
        type="text"
        aria-label="Folder"
        spellcheck="false"
        autocomplete="off"
        bind:value={typed}
        onkeydown={onPathKey}
      />
    </div>

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    <ul class="dlg-list" class:loading>
      {#each shown as entry (entry.path)}
        <li>
          <button class="dlg-row entry" onclick={() => choose(entry)}>
            <img
              src={entry.dir ? folderIcon(entry.name, false) : fileIcon(entry.name)}
              alt=""
              width="14"
              height="14"
              draggable="false"
            />
            <span class="dlg-label">{entry.name}</span>
          </button>
        </li>
      {:else}
        <li class="dlg-empty">{loading ? "Reading…" : "Nothing here."}</li>
      {/each}
    </ul>

    {#if request.mode === "save"}
      <div class="bar name">
        <label for="browse-name">Name</label>
        <input
          id="browse-name"
          class="path"
          type="text"
          spellcheck="false"
          autocomplete="off"
          bind:value={name}
          onkeydown={(e) => e.key === "Enter" && confirm()}
        />
      </div>
    {/if}

    <div class="dlg-actions">
      <button class="btn ghost" onclick={() => finish(null)}>Cancel</button>
      {#if request.mode === "folder"}
        <button class="btn primary" disabled={path === ""} onclick={confirm}>Open This Folder</button>
      {:else if request.mode === "save"}
        <button class="btn primary" disabled={name.trim() === ""} onclick={confirm}>Save</button>
      {/if}
    </div>
  {/if}
</Dialog>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-bottom: 0.5rem;
  }
  .bar.name {
    margin: 0.6rem 0 0;
  }
  .bar label {
    font-size: 0.8rem;
    color: var(--fg-dim);
  }

  .up {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    background: transparent;
    border: 1px solid var(--border-input);
    border-radius: 4px;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .up:hover:not(:disabled) {
    color: var(--fg);
    border-color: var(--fg-dim);
  }
  .up:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .path {
    flex: 1;
    min-width: 0;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 4px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.8rem;
    padding: 0.3rem 0.45rem;
  }
  .path:focus-visible {
    outline: none;
    border-color: var(--accent);
  }

  /* The listing keeps its height while another loads, so the dialog does not
     jump; it only dims. */
  .dlg-list {
    min-height: 220px;
    transition: opacity 120ms ease;
  }
  .dlg-list.loading {
    opacity: 0.55;
  }

  .entry {
    width: 100%;
    background: transparent;
    border: none;
    color: var(--fg);
    cursor: pointer;
    font-family: inherit;
    text-align: left;
    padding: 0.28rem 0.5rem;
  }
  .entry img {
    flex: none;
  }
  .entry .dlg-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error {
    margin: 0 0 0.5rem;
    font-size: 0.78rem;
    color: var(--danger);
    overflow-wrap: anywhere;
  }
</style>
