<script lang="ts">
  /**
   * The tmux sessions on the remote, where the file tree is in the editor.
   *
   * A click goes to a session. Starting one, renaming one and ending one are
   * all here too, as buttons: there are no keys for any of it.
   */
  import type { SessionInfo } from "./tmux.svelte";

  type Props = {
    sessions: SessionInfo[];
    /** The session attached to. */
    activeId: number | null;
    /** Whether a session can be started: there is a remote, and it has tmux. */
    usable: boolean;
    /**
     * Whether the window is attached to tmux. Renaming a session and ending
     * one are asked of tmux, which only an attachment can do.
     */
    attached: boolean;
    width: number;
    collapsed: boolean;
    /** A drag is in flight, so the width must track the pointer, not glide. */
    resizing: boolean;
    onselect: (session: number) => void;
    /** Start a session. With no name, tmux numbers it. */
    onnew: (name: string) => void;
    onrename: (session: number, name: string) => void;
    /** End a session and everything running in it. */
    onkill: (session: number) => void;
    /** Right-click on a session. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, session: number) => void;
  };

  let {
    sessions,
    activeId,
    usable,
    attached,
    width,
    collapsed,
    resizing,
    onselect,
    onnew,
    onrename,
    onkill,
    oncontext,
  }: Props = $props();

  // --- the name box --------------------------------------------------------------

  /** What is being named: a session there is, or one about to be started. */
  let naming = $state<number | "new" | null>(null);
  /** Enter settles the box and removes it, and removing it blurs it: one answer. */
  let settled = false;

  /** Type a new name onto a session's row. */
  export function rename(session: number) {
    if (attached) naming = session;
  }

  function settle(value: string | null) {
    if (settled) return;
    settled = true;
    const what = naming;
    naming = null;
    if (what === null || value === null) return;
    const name = value.trim();
    if (what === "new") onnew(name);
    else if (name !== "" && name !== sessions.find((each) => each.id === what)?.name) {
      onrename(what, name);
    }
  }

  function nameBox(node: HTMLInputElement) {
    settled = false;
    node.focus();
    node.select();
    node.scrollIntoView({ block: "nearest" });
  }

  function onNameKey(event: KeyboardEvent) {
    event.stopPropagation();
    if (event.key === "Enter") settle((event.currentTarget as HTMLInputElement).value);
    else if (event.key === "Escape") settle(null);
  }
</script>

<aside style="width: {width}px" class:collapsed class:resizing>
  <div class="content" inert={collapsed}>
    <header>
      <span class="heading">Sessions</span>
      <button
        class="add"
        title="New session"
        aria-label="New session"
        disabled={!usable}
        onclick={() => (naming = "new")}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M5 1 V9 M1 5 H9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
        </svg>
      </button>
    </header>

    <ul>
      {#each sessions as session (session.id)}
        <li>
          <div
            class="row"
            class:active={session.id === activeId}
            role="button"
            tabindex="0"
            title={naming === session.id
              ? undefined
              : `${session.name}: ${session.windows} ${session.windows === 1 ? "window" : "windows"}`}
            onclick={() => naming !== session.id && onselect(session.id)}
            ondblclick={() => rename(session.id)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") onselect(session.id);
            }}
            oncontextmenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              if (attached && naming !== session.id) oncontext(e, session.id);
            }}
          >
            <!-- Lit for a session something is attached to, this window or
                 another terminal somewhere. -->
            <span class="mark" class:attached={session.attached > 0}></span>
            {#if naming === session.id}
              <input
                class="name"
                value={session.name}
                spellcheck="false"
                autocomplete="off"
                aria-label="Session name"
                use:nameBox
                onkeydown={onNameKey}
                onclick={(e) => e.stopPropagation()}
                ondblclick={(e) => e.stopPropagation()}
                onblur={(e) => settle(e.currentTarget.value)}
              />
            {:else}
              <span class="label">{session.name}</span>
              <span class="count">{session.windows}</span>
              <button
                class="kill"
                class:never={!attached}
                title="Kill session"
                aria-label="Kill session {session.name}"
                tabindex="-1"
                onclick={(e) => {
                  e.stopPropagation();
                  onkill(session.id);
                }}
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
            {/if}
          </div>
        </li>
      {/each}

      {#if naming === "new"}
        <li>
          <div class="row">
            <span class="mark"></span>
            <input
              class="name"
              placeholder="Name, or Enter for none"
              spellcheck="false"
              autocomplete="off"
              aria-label="New session name"
              use:nameBox
              onkeydown={onNameKey}
              onblur={(e) => settle(e.currentTarget.value.trim() === "" ? null : e.currentTarget.value)}
            />
          </div>
        </li>
      {/if}
    </ul>
  </div>
</aside>

<style>
  /* Sized, collapsed and eased as the file tree is (`Sidebar`): it is the
     same strip of the window, showing something else. */
  aside {
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    transition: width 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
  }
  aside.resizing {
    transition: none;
  }

  .content {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
    transition: opacity 140ms ease;
  }
  .collapsed .content {
    opacity: 0;
    transition-duration: 90ms;
  }

  @media (prefers-reduced-motion: reduce) {
    aside,
    .content {
      transition: none;
    }
  }

  header {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 26px;
    padding: 0 0.35rem 0 0.8rem;
    user-select: none;
  }
  .heading {
    color: var(--fg-dim);
    font-size: 0.68rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .add,
  .kill {
    flex: none;
    display: grid;
    place-items: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .add {
    width: 20px;
    height: 18px;
  }
  .add:hover:not(:disabled) {
    background: var(--hover);
    color: var(--fg);
  }
  .add:disabled {
    opacity: 0.35;
    cursor: default;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0 0.35rem 0.3rem;
    flex: 1;
    min-height: 0;
    overflow-x: hidden;
    overflow-y: auto;
  }

  .row {
    display: flex;
    align-items: center;
    min-height: 24px;
    padding: 0 0.3rem 0 0.45rem;
    border-radius: var(--chip-radius);
    cursor: pointer;
    user-select: none;
  }
  .row:hover {
    background: var(--hover);
  }
  .row:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .row.active {
    background: var(--accent-soft);
  }

  .mark {
    flex: none;
    width: 6px;
    height: 6px;
    margin-right: 0.55rem;
    border-radius: 50%;
    background: var(--fg-faint);
  }
  .mark.attached {
    background: var(--accent);
  }

  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.82rem;
    color: var(--fg);
  }

  /* How many windows it has: one slot at the row's end, which the button
     that ends the session takes while the pointer is on the row. */
  .count {
    flex: none;
    min-width: 16px;
    text-align: center;
    color: var(--fg-dim);
    font-size: 0.7rem;
  }
  .kill {
    display: none;
    width: 16px;
    height: 16px;
  }
  .row:hover .count:not(:has(+ .never)) {
    display: none;
  }
  .row:hover .kill:not(.never) {
    display: grid;
  }
  .kill:hover {
    background: #f38ba81f;
    color: var(--danger);
  }

  .name {
    flex: 1;
    min-width: 0;
    height: 18px;
    padding: 0 0.25rem;
    margin-left: -0.25rem;
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: 3px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.82rem;
    outline: none;
  }
</style>
