<script lang="ts">
  /**
   * Edit → LSPs: the language servers found on the machine the window is
   * working on (this one, or its remote), and which are switched on.
   *
   * Switching one on writes it into `[lsp] enabled` in the config, the same
   * as adding it there by hand. Tide installs nothing, and lists nothing
   * the machine does not have.
   */
  import Dialog from "./Dialog.svelte";
  import SyntaxTab from "./SyntaxTab.svelte";
  import type { ConfigStore } from "./state/config.svelte";
  import type { Lsp, ServerInfo } from "./state/lsp.svelte";

  type Props = {
    open: boolean;
    lsp: Lsp;
    config: ConfigStore;
    /** The remote the window is on, which is then where they run; null for this machine. */
    host: string | null;
    onclose: () => void;
  };

  let { open, lsp, config, host, onclose }: Props = $props();

  /** Which of the two this is showing: the servers, or the user's own languages. */
  let tab = $state<"servers" | "languages">("servers");

  // Each time it opens: one may have been installed since the last look.
  $effect(() => {
    if (open) void lsp.refresh();
  });

  // Only what the machine has, in the order of the catalog.
  const servers = $derived(lsp.catalog.filter((server) => server.installed));

  function toggle(server: ServerInfo, on: boolean) {
    const rest = lsp.enabled.filter((id) => id !== server.id);
    void config.set("lsp", "enabled", on ? [...rest, server.id] : rest);
  }

  /** What a server is doing, in a word, and whether that word is bad news. */
  function status(server: ServerInfo): { text: string; bad?: boolean } | null {
    if (!lsp.enabled.includes(server.id)) return null;
    const state = lsp.states[server.id];
    if (state === "running") return { text: "Running" };
    if (state === "starting") return { text: "Starting…" };
    if (state === "failed") return { text: "Failed", bad: true };
    // Switched on, and no file of its languages is open yet.
    return { text: "Ready" };
  }
</script>

<Dialog {open} title="Languages" width={tab === "servers" ? 580 : 940} scrolls {onclose}>
  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={tab === "servers"} onclick={() => (tab = "servers")}>
      Language Servers
    </button>
    <button role="tab" aria-selected={tab === "languages"} onclick={() => (tab = "languages")}>
      Your Languages
    </button>
  </div>

  {#if tab === "languages"}
    <SyntaxTab {config} />
    <div class="dlg-actions">
      <button class="btn ghost" onclick={onclose}>Done</button>
    </div>
  {:else}
  <ul class="dlg-list">
    {#each servers as server (server.id)}
      {@const state = status(server)}
      {@const on = lsp.enabled.includes(server.id)}
      <li>
        <label class="dlg-row">
          <input
            type="checkbox"
            checked={on}
            onchange={(e) => toggle(server, e.currentTarget.checked)}
          />
          <span class="dlg-detail">
            <span class="dlg-label">{server.name}</span>
            <span class="dlg-meta">{server.languages.map((l) => l.name).join(", ")}</span>
            {#if on && lsp.errors[server.id]}
              <span class="dlg-meta why">{lsp.errors[server.id]}</span>
            {/if}
          </span>
          {#if state}
            <span class="state" class:bad={state.bad}>{state.text}</span>
          {/if}
          {#if on && lsp.states[server.id] === "failed"}
            <button
              class="btn ghost retry"
              onclick={(e) => {
                e.preventDefault();
                lsp.retry(server.id);
              }}
            >
              Retry
            </button>
          {/if}
        </label>
      </li>
    {:else}
      <li class="dlg-empty">No language servers were found on {host ?? "this machine"}.</li>
    {/each}
  </ul>

  <p class="dlg-note">
    These are the language servers found on the PATH of {host ?? "this machine"}, which is
    where they run. One that is switched on starts with the first file in one of its languages,
    and gives completion as you type and its errors at the end of the line they are on. Tide
    installs none itself.
  </p>

  <div class="dlg-actions">
    <button class="btn primary" onclick={onclose}>Done</button>
  </div>
  {/if}
</Dialog>

<style>
  /* The two halves of the dialog, as a pair of tabs under its title. */
  .tabs {
    flex: none;
    display: flex;
    gap: 1.1rem;
    margin: -0.2rem 0 0.8rem;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    padding: 0.3rem 0;
    margin-bottom: -1px;
    background: transparent;
    border: none;
    border-bottom: 1px solid transparent;
    color: var(--fg-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }
  .tabs button:hover {
    color: var(--fg);
  }
  .tabs button[aria-selected="true"] {
    border-bottom-color: var(--accent);
    color: var(--fg);
  }

  .dlg-row {

    cursor: pointer;
  }

  input[type="checkbox"] {
    flex: none;
    accent-color: var(--accent);
  }

  .why {
    color: var(--danger);
    white-space: normal;
  }

  .state {
    flex: none;
    color: var(--fg-dim);
    font-size: 0.72rem;
  }
  .state.bad {
    color: var(--danger);
  }

  .retry {
    flex: none;
    padding: 0.15rem 0.5rem;
    font-size: 0.72rem;
  }
</style>
