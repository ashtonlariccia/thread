<script lang="ts">
  /**
   * Edit → LSPs: the language servers Thread knows how to run, which of them
   * are installed, and which are switched on.
   *
   * Switching one on writes it into `[lsp] enabled` in the config, the same
   * as adding it there by hand. Thread installs nothing: a server that is
   * not on this machine is listed with the command that would put it there.
   */
  import Dialog from "./Dialog.svelte";
  import type { ConfigStore } from "./state/config.svelte";
  import type { Lsp, ServerInfo } from "./state/lsp.svelte";

  type Props = {
    open: boolean;
    lsp: Lsp;
    config: ConfigStore;
    /** Whether the window is on a remote, where none of them run. */
    remote: boolean;
    onclose: () => void;
  };

  let { open, lsp, config, remote, onclose }: Props = $props();

  // Each time it opens: one may have been installed since the last look.
  $effect(() => {
    if (open) void lsp.refresh();
  });

  // The installed ones first, each lot in the order of the catalog.
  const servers = $derived([
    ...lsp.catalog.filter((server) => server.installed),
    ...lsp.catalog.filter((server) => !server.installed),
  ]);

  function toggle(server: ServerInfo, on: boolean) {
    const rest = lsp.enabled.filter((id) => id !== server.id);
    void config.set("lsp", "enabled", on ? [...rest, server.id] : rest);
  }

  /** What a server is doing, in a word, and whether that word is bad news. */
  function status(server: ServerInfo): { text: string; bad?: boolean } | null {
    if (!server.installed) return { text: "Not installed" };
    if (!lsp.enabled.includes(server.id)) return null;
    if (remote) return { text: "Not on a remote" };
    const state = lsp.states[server.id];
    if (state === "running") return { text: "Running" };
    if (state === "starting") return { text: "Starting…" };
    if (state === "failed") return { text: "Failed", bad: true };
    // Switched on, and no file of its languages is open yet.
    return { text: "Ready" };
  }
</script>

<Dialog {open} title="Language Servers" width={580} scrolls {onclose}>
  <ul class="dlg-list">
    {#each servers as server (server.id)}
      {@const state = status(server)}
      {@const on = lsp.enabled.includes(server.id)}
      <li>
        <label class="dlg-row" class:missing={!server.installed}>
          <input
            type="checkbox"
            checked={on}
            disabled={!server.installed && !on}
            onchange={(e) => toggle(server, e.currentTarget.checked)}
          />
          <span class="dlg-detail">
            <span class="dlg-label">{server.name}</span>
            <span class="dlg-meta">{server.languages.map((l) => l.name).join(", ")}</span>
            {#if !server.installed}
              <!-- To be selected and copied, which a label would swallow. -->
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
              <code class="install" onclick={(e) => e.preventDefault()}>{server.install}</code>
            {:else if on && lsp.errors[server.id]}
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
    {/each}
  </ul>

  <p class="dlg-note">
    A server that is switched on starts with the first file in one of its languages, and gives
    completion as you type and its errors at the end of the line they are on. Thread runs the
    ones it finds on your PATH and installs none itself.
    {#if remote}
      They run on this machine only: this window is on a remote, and has none for now.
    {/if}
  </p>

  <div class="dlg-actions">
    <button class="btn primary" onclick={onclose}>Done</button>
  </div>
</Dialog>

<style>
  .dlg-row {
    cursor: pointer;
  }
  .dlg-row.missing {
    cursor: default;
  }
  .dlg-row.missing .dlg-label {
    color: var(--fg-dim);
  }

  input[type="checkbox"] {
    flex: none;
    accent-color: var(--accent);
  }

  .install {
    align-self: flex-start;
    margin-top: 0.15rem;
    padding: 0.05rem 0.35rem;
    background: var(--bg-input);
    border-radius: 3px;
    color: var(--fg-dim);
    font-family: Consolas, monospace;
    font-size: 0.7rem;
    cursor: text;
    user-select: text;
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
