<script lang="ts">
  /**
   * The remote's tmux, where the file tree is in the editor: each session,
   * under it its windows, and under each window what is running in it.
   *
   * A click goes to a session or a window. Starting, renaming and ending
   * them are all here too, as buttons: there are no keys for any of it.
   */
  import { iconUrl } from "../icons";
  import { jobs, type Process, type SessionInfo, type WindowBrief } from "./tmux.svelte";

  type Props = {
    sessions: SessionInfo[];
    /** The session attached to. */
    activeId: number | null;
    /** The window of that session that is in front. */
    frontWindow: number | null;
    /** Every process on one of the remote's terminals, as last read. */
    processes: Process[];
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
    /** Go to a window, and to its session if that is another. */
    onwindow: (session: number, window: number) => void;
    /** Go to the pane something is running in. */
    onjob: (session: number, window: number, pane: number) => void;
    /** Start a session. With no name, tmux numbers it. */
    onnew: (name: string) => void;
    onnewwindow: (session: number) => void;
    onrename: (session: number, name: string) => void;
    onrenamewindow: (window: number, name: string) => void;
    /** End a session and everything running in it. */
    onkill: (session: number) => void;
    onclosewindow: (session: number, window: number) => void;
    /** Right-click on a row. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, session: number) => void;
    onwindowcontext: (event: MouseEvent, session: number, window: number) => void;
  };

  let {
    sessions,
    activeId,
    frontWindow,
    processes,
    usable,
    attached,
    width,
    collapsed,
    resizing,
    onselect,
    onwindow,
    onjob,
    onnew,
    onnewwindow,
    onrename,
    onrenamewindow,
    onkill,
    onclosewindow,
    oncontext,
    onwindowcontext,
  }: Props = $props();

  /** The sessions folded shut. Open is how one starts: its windows are the point of it. */
  let folded = $state<number[]>([]);

  function fold(session: number) {
    folded = folded.includes(session)
      ? folded.filter((each) => each !== session)
      : [...folded, session];
  }

  /** A share of a core, as the bar at the bottom says the whole machine's. */
  const percent = (cpu: number) => `${Math.round(cpu)}%`;

  // --- the name box --------------------------------------------------------------

  /** What is being named: a session or a window there is, or a session about to be. */
  type Naming = { kind: "session" | "window"; id: number } | { kind: "new" };
  let naming = $state<Naming | null>(null);
  /** Enter settles the box and removes it, and removing it blurs it: one answer. */
  let settled = false;

  const namingSession = (id: number) => naming?.kind === "session" && naming.id === id;
  const namingWindow = (id: number) => naming?.kind === "window" && naming.id === id;

  /** Type a new name onto a session's row. */
  export function rename(session: number) {
    if (attached) naming = { kind: "session", id: session };
  }

  /** Type a new name onto a window's row. */
  export function renameWindow(window: number) {
    if (attached) naming = { kind: "window", id: window };
  }

  function settle(value: string | null, was = "") {
    if (settled) return;
    settled = true;
    const what = naming;
    naming = null;
    if (what === null || value === null) return;
    const name = value.trim();
    if (what.kind === "new") onnew(name);
    else if (name === "" || name === was) return;
    else if (what.kind === "session") onrename(what.id, name);
    else onrenamewindow(what.id, name);
  }

  function nameBox(node: HTMLInputElement) {
    settled = false;
    node.focus();
    node.select();
    node.scrollIntoView({ block: "nearest" });
  }

  function onNameKey(event: KeyboardEvent, was = "") {
    event.stopPropagation();
    if (event.key === "Enter") settle((event.currentTarget as HTMLInputElement).value, was);
    else if (event.key === "Escape") settle(null);
  }

  const windowsOf = (session: SessionInfo): WindowBrief[] =>
    folded.includes(session.id) ? [] : session.list;
</script>

{#snippet cross()}
  <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
    <path d="M1.5 1.5 L8.5 8.5 M8.5 1.5 L1.5 8.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
  </svg>
{/snippet}

{#snippet plus()}
  <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
    <path d="M5 1 V9 M1 5 H9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
  </svg>
{/snippet}

<aside style="width: {width}px" class:collapsed class:resizing>
  <div class="content" inert={collapsed}>
    <header>
      <span class="heading">Sessions</span>
      <button
        class="tool"
        title="New session"
        aria-label="New session"
        disabled={!usable}
        onclick={() => (naming = { kind: "new" })}
      >
        {@render plus()}
      </button>
    </header>

    <ul>
      {#each sessions as session (session.id)}
        {@const open = !folded.includes(session.id)}
        <li>
          <div
            class="row session"
            class:active={session.id === activeId}
            role="button"
            tabindex="0"
            aria-expanded={session.list.length > 0 ? open : undefined}
            title={namingSession(session.id)
              ? undefined
              : `${session.name}: ${session.windows} ${session.windows === 1 ? "window" : "windows"}`}
            onclick={() => !namingSession(session.id) && onselect(session.id)}
            ondblclick={() => rename(session.id)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") onselect(session.id);
            }}
            oncontextmenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              if (attached && !namingSession(session.id)) oncontext(e, session.id);
            }}
          >
            <!-- Folds its windows away. Its own button, so that going to a
                 session and tidying the list are not the same click. -->
            <button
              class="chevron"
              class:open
              tabindex="-1"
              aria-label={open ? "Fold" : "Unfold"}
              disabled={session.list.length === 0}
              onclick={(e) => {
                e.stopPropagation();
                fold(session.id);
              }}
            >
              <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
                <path
                  d="M3.5 2 L6.5 5 L3.5 8"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.3"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            </button>
            {#if namingSession(session.id)}
              <input
                class="name"
                value={session.name}
                spellcheck="false"
                autocomplete="off"
                aria-label="Session name"
                use:nameBox
                onkeydown={(e) => onNameKey(e, session.name)}
                onclick={(e) => e.stopPropagation()}
                ondblclick={(e) => e.stopPropagation()}
                onblur={(e) => settle(e.currentTarget.value, session.name)}
              />
            {:else}
              <span class="label">{session.name}</span>
              <!-- Lit for a session something is attached to, this window
                   or another terminal somewhere. -->
              <span class="mark" class:attached={session.attached > 0}></span>
              {#if attached}
                <button
                  class="tool hover"
                  title="New window in {session.name}"
                  aria-label="New window in {session.name}"
                  tabindex="-1"
                  onclick={(e) => {
                    e.stopPropagation();
                    onnewwindow(session.id);
                  }}
                >
                  {@render plus()}
                </button>
                <button
                  class="tool hover danger"
                  title="Kill session"
                  aria-label="Kill session {session.name}"
                  tabindex="-1"
                  onclick={(e) => {
                    e.stopPropagation();
                    onkill(session.id);
                  }}
                >
                  {@render cross()}
                </button>
              {/if}
            {/if}
          </div>
        </li>

        {#each windowsOf(session) as window (window.id)}
          {@const running = jobs(window.panes, processes)}
          <li>
            <div
              class="row window"
              class:active={session.id === activeId && window.id === frontWindow}
              role="button"
              tabindex="0"
              title={namingWindow(window.id) ? undefined : `${window.index}: ${window.name}`}
              onclick={() => !namingWindow(window.id) && onwindow(session.id, window.id)}
              ondblclick={() => renameWindow(window.id)}
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ") onwindow(session.id, window.id);
              }}
              onauxclick={(e) => {
                // Middle-click closes, as it does on a tab.
                if (e.button !== 1) return;
                e.preventDefault();
                onclosewindow(session.id, window.id);
              }}
              onmousedown={(e) => {
                // Stops the middle button starting the webview's autoscroll.
                if (e.button === 1) e.preventDefault();
              }}
              oncontextmenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                if (!namingWindow(window.id)) onwindowcontext(e, session.id, window.id);
              }}
            >
              <img src={iconUrl("console")} alt="" width="14" height="14" draggable="false" />
              {#if namingWindow(window.id)}
                <input
                  class="name"
                  value={window.name}
                  spellcheck="false"
                  autocomplete="off"
                  aria-label="Window name"
                  use:nameBox
                  onkeydown={(e) => onNameKey(e, window.name)}
                  onclick={(e) => e.stopPropagation()}
                  ondblclick={(e) => e.stopPropagation()}
                  onblur={(e) => settle(e.currentTarget.value, window.name)}
                />
              {:else}
                <span class="label">{window.name}</span>
                <button
                  class="tool hover danger"
                  title="Close window"
                  aria-label="Close window {window.name}"
                  tabindex="-1"
                  onclick={(e) => {
                    e.stopPropagation();
                    onclosewindow(session.id, window.id);
                  }}
                >
                  {@render cross()}
                </button>
              {/if}
            </div>
          </li>

          <!-- What was run in it: a build, a server. For reading, and for
               going to the pane it is in. -->
          {#each running as job (job.pid)}
            <li>
              <div
                class="row job"
                role="button"
                tabindex="0"
                title={job.command}
                onclick={() => onjob(session.id, window.id, job.pane)}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") onjob(session.id, window.id, job.pane);
                }}
              >
                <span class="label">{job.command}</span>
                {#if job.children > 0}
                  <span class="more" title="Processes it has started">+{job.children}</span>
                {/if}
                <span class="cpu" class:working={job.cpu >= 50} title="Of one core">{percent(job.cpu)}</span>
              </div>
            </li>
          {/each}
        {/each}
      {/each}

      {#if naming?.kind === "new"}
        <li>
          <div class="row session">
            <span class="chevron"></span>
            <input
              class="name"
              placeholder="Name, or Enter for none"
              spellcheck="false"
              autocomplete="off"
              aria-label="New session name"
              use:nameBox
              onkeydown={(e) => onNameKey(e)}
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
    gap: 0.35rem;
    min-height: 22px;
    padding: 0 0.3rem 0 0.2rem;
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
  /* The window in front, and the session it is of: the one filled, the
     other only marked, so that it is the window the eye lands on. */
  .row.window.active {
    background: var(--accent-soft);
  }
  .row.session.active .label {
    color: var(--accent);
  }

  /* Each level steps in by the width of the chevron above it. */
  .row.session {
    min-height: 24px;
  }
  .row.window {
    padding-left: calc(0.2rem + 16px);
  }
  .row.job {
    min-height: 19px;
    padding-left: calc(0.2rem + 16px + 14px + 0.35rem);
  }

  .chevron {
    flex: none;
    display: grid;
    place-items: center;
    width: 14px;
    height: 14px;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .chevron:disabled {
    visibility: hidden;
  }
  .chevron svg {
    transition: transform 90ms ease;
  }
  .chevron.open svg {
    transform: rotate(90deg);
  }

  .row img {
    flex: none;
    display: block;
  }

  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.82rem;
    color: var(--fg);
  }
  .session .label {
    font-weight: 600;
  }
  .job .label,
  .more,
  .cpu {
    font-size: 0.72rem;
    color: var(--fg-dim);
  }
  .more,
  .cpu {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .cpu {
    min-width: 2.6em;
    text-align: right;
  }
  /* Something that is working, as against something that is waiting. */
  .cpu.working {
    color: var(--accent);
  }

  .mark {
    flex: none;
    width: 6px;
    height: 6px;
    margin: 0 0.25rem;
    border-radius: 50%;
    background: var(--fg-faint);
  }
  .mark.attached {
    background: var(--accent);
  }

  .tool {
    flex: none;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .tool:hover:not(:disabled) {
    background: var(--hover);
    color: var(--fg);
  }
  .tool:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .tool.danger:hover {
    background: #f38ba81f;
    color: var(--danger);
  }
  /* There on the row the pointer is on, and nowhere else: a column of
     buttons down the list would be most of what the list showed. In their
     place, on a session, is the mark that says something is attached. */
  .tool.hover {
    display: none;
  }
  .row:hover .tool.hover {
    display: grid;
  }
  .row:hover .mark {
    display: none;
  }

  .name {
    flex: 1;
    min-width: 0;
    height: 18px;
    padding: 0 0.25rem;
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: 3px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.82rem;
    outline: none;
  }
</style>
