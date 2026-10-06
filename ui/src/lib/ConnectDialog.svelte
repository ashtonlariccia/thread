<script lang="ts">
  /**
   * Remote → Connect: who to connect to, and how to sign in.
   *
   * The fields outlive a failed attempt. A password the server turned down
   * should cost retyping the password, not the whole form.
   */
  import Dialog from "./Dialog.svelte";
  import type { ConnectRequest, DiscoveredKey } from "./state/remote.svelte";

  type Props = {
    open: boolean;
    /** Private keys found in `~\.ssh`, to choose from. */
    keys: DiscoveredKey[];
    /** An attempt is in flight. */
    busy: boolean;
    /** Why the last attempt failed, shown under the fields. */
    error: string | null;
    onsubmit: (request: ConnectRequest) => void;
    oncancel: () => void;
  };

  let { open, keys, busy, error, onsubmit, oncancel }: Props = $props();

  let host = $state("");
  let port = $state(22);
  let username = $state("");
  let method = $state<"password" | "key">("password");
  let password = $state("");

  /** Stands for "a key that is somewhere else", whose path is typed. */
  const OTHER = "";
  let keyChoice = $state(OTHER);
  let keyPath = $state("");
  let passphrase = $state("");

  const chosenKey = $derived(keys.find((key) => key.path === keyChoice));
  const ready = $derived(
    host.trim() !== "" &&
      username.trim() !== "" &&
      port >= 1 &&
      port <= 65535 &&
      (method === "password" || (keyChoice === OTHER ? keyPath.trim() !== "" : true)),
  );

  let hostInput = $state<HTMLInputElement | undefined>();
  let passwordInput = $state<HTMLInputElement | undefined>();

  // The address first; and after a refusal, the thing most likely wrong.
  $effect(() => {
    if (open) hostInput?.focus();
  });
  $effect(() => {
    if (error && method === "password") passwordInput?.select();
  });

  // Secrets do not sit in a closed dialog waiting for the next time it opens.
  $effect(() => {
    if (!open) {
      password = "";
      passphrase = "";
    }
  });

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!ready || busy) return;
    onsubmit({
      host: host.trim(),
      port,
      username: username.trim(),
      auth:
        method === "password"
          ? { kind: "password", password }
          : { kind: "key", path: keyChoice === OTHER ? keyPath.trim() : keyChoice, passphrase },
    });
  }
</script>

<!-- Not dismissed while an attempt is in flight: the answer has to land
     somewhere. -->
<Dialog {open} title="Connect to a remote" width={440} onclose={busy ? null : oncancel}>
  <form onsubmit={submit}>
    <div class="grid">
      <label for="remote-host">Host</label>
      <div class="pair">
        <input
          id="remote-host"
          type="text"
          placeholder="hostname or address"
          autocomplete="off"
          spellcheck="false"
          bind:value={host}
          bind:this={hostInput}
          disabled={busy}
        />
        <input
          class="port"
          type="number"
          min="1"
          max="65535"
          aria-label="Port"
          title="Port"
          bind:value={port}
          disabled={busy}
        />
      </div>

      <label for="remote-user">User</label>
      <input
        id="remote-user"
        type="text"
        autocomplete="off"
        spellcheck="false"
        bind:value={username}
        disabled={busy}
      />

      <span class="label">Sign in with</span>
      <div class="methods">
        <label class="choice">
          <input type="radio" value="password" bind:group={method} disabled={busy} />
          Password
        </label>
        <label class="choice">
          <input type="radio" value="key" bind:group={method} disabled={busy} />
          Key
        </label>
      </div>

      {#if method === "password"}
        <label for="remote-password">Password</label>
        <input
          id="remote-password"
          type="password"
          bind:value={password}
          bind:this={passwordInput}
          disabled={busy}
        />
      {:else}
        <label for="remote-key">Key</label>
        <select id="remote-key" bind:value={keyChoice} disabled={busy}>
          {#each keys as key (key.path)}
            <option value={key.path}>{key.name}</option>
          {/each}
          <option value={OTHER}>Another file…</option>
        </select>

        {#if keyChoice === OTHER}
          <label for="remote-key-path">Path</label>
          <input
            id="remote-key-path"
            type="text"
            placeholder="C:\Users\you\.ssh\id_ed25519"
            autocomplete="off"
            spellcheck="false"
            bind:value={keyPath}
            disabled={busy}
          />
        {/if}

        <!-- Always for a typed path, which cannot be looked at first; for a
             found key, only when it is known to need one. -->
        {#if keyChoice === OTHER || chosenKey?.encrypted}
          <label for="remote-passphrase">Passphrase</label>
          <input
            id="remote-passphrase"
            type="password"
            placeholder={keyChoice === OTHER ? "if the key has one" : ""}
            bind:value={passphrase}
            disabled={busy}
          />
        {/if}
      {/if}
    </div>

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    <div class="dlg-actions">
      <button type="button" class="btn ghost" onclick={oncancel} disabled={busy}>Cancel</button>
      <button type="submit" class="btn primary" disabled={!ready || busy}>
        {busy ? "Connecting…" : "Connect"}
      </button>
    </div>
  </form>
</Dialog>

<style>
  .grid {
    display: grid;
    grid-template-columns: max-content 1fr;
    align-items: center;
    gap: 0.5rem 0.9rem;
  }

  label,
  .label {
    font-size: 0.8rem;
    color: var(--fg-dim);
  }

  .pair {
    display: flex;
    gap: 0.4rem;
  }
  .pair input:first-child {
    flex: 1;
    min-width: 0;
  }
  .port {
    width: 4.6rem;
  }

  .methods {
    display: flex;
    gap: 1.1rem;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--fg);
    cursor: pointer;
  }
  .choice input {
    accent-color: var(--accent);
    margin: 0;
  }

  input[type="text"],
  input[type="password"],
  input[type="number"],
  select {
    min-width: 0;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 4px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.8rem;
    padding: 0.3rem 0.45rem;
  }
  input:focus-visible,
  select:focus-visible {
    outline: none;
    border-color: var(--accent);
  }
  input:disabled,
  select:disabled {
    opacity: 0.6;
  }

  .error {
    margin: 0.8rem 0 0;
    font-size: 0.78rem;
    line-height: 1.4;
    color: var(--danger);
    /* A message can carry a path or a fingerprint, each one long word. */
    overflow-wrap: anywhere;
  }
</style>
