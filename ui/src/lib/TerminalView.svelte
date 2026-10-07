<script lang="ts">
  /**
   * One terminal, filling the space a file's text would.
   *
   * Mounted for as long as its tab is open, shown or not: switching to another
   * tab hides it with everything in it still running, and unmounting it is
   * what ends the shell.
   */
  import { onMount } from "svelte";

  import type { Link } from "./links";
  import type { OpenKind, TerminalHandle, TerminalLook } from "./terminal";

  type Props = {
    /** Whether this is the tab being shown. The shell runs either way. */
    active: boolean;
    /** Whether it is being shown in the pane the keyboard is in. */
    focused: boolean;
    /** Where the shell starts. Read once, when it does. */
    cwd: string | null;
    /** The command line of the shell to run; null for the config's. Read once. */
    shell: string | null;
    look: TerminalLook;
    scrollback: number;
    /** The shell ended by itself. */
    onexit: () => void;
    /** The `thread` command was run in it, on a file or a folder. */
    onopen: (kind: OpenKind, path: string) => void;
    /** A path or a web address in its output was Ctrl+clicked. */
    onlink: (link: Link) => void;
    oncontext: (event: MouseEvent) => void;
  };

  let { active, focused, cwd, shell, look, scrollback, onexit, onopen, onlink, oncontext }: Props =
    $props();

  let host: HTMLElement;
  let handle = $state.raw<TerminalHandle | null>(null);

  export function focus() {
    handle?.focus();
  }
  export function hasSelection(): boolean {
    return handle?.hasSelection() ?? false;
  }
  export function copy() {
    handle?.copy();
  }
  export function paste() {
    handle?.paste();
  }

  onMount(() => {
    let gone = false;

    // Loaded here, the first time a terminal is wanted, rather than with the
    // window: see `terminal.ts`.
    void import("./terminal").then(({ openTerminal }) => {
      if (gone) return;
      handle = openTerminal(host, {
        cwd,
        shell,
        ...look,
        scrollback,
        onexit,
        onopen,
        onlink,
        onfind: () => (finding = true),
      });
    });

    return () => {
      gone = true;
      handle?.dispose();
    };
  });

  // Coming to a terminal's tab, or to the pane it is showing in, is asking
  // to type in it.
  $effect(() => {
    if (focused) handle?.focus();
  });


  $effect(() => {
    handle?.restyle(look);
  });

  // --- searching the output ---------------------------------------------------
  //
  // Ctrl+Shift+F puts a field in the corner of the terminal. Each match is
  // selected as it is gone to, which is both how it is shown and how it is
  // copied.

  let finding = $state(false);
  let wanted = $state("");
  /** Whether the last thing looked for was there to find. */
  let missing = $state(false);
  let field = $state<HTMLInputElement | undefined>();

  export function find() {
    finding = true;
    field?.select();
  }

  $effect(() => {
    if (finding) field?.select();
  });

  function lookFor(back: boolean) {
    missing = wanted !== "" && !(handle?.find(wanted, back) ?? false);
  }

  function onFindKey(event: KeyboardEvent) {
    if (event.key === "Enter") lookFor(event.shiftKey);
    else if (event.key === "Escape") {
      finding = false;
      handle?.focus();
    } else return;
    event.preventDefault();
    event.stopPropagation();
  }

  // In the capture phase, ahead of xterm: a program reading the mouse would
  // otherwise be sent the right-click as a click of its own.
  function onContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    oncontext(event);
  }
</script>

<!-- Hidden rather than removed while another tab is showing, so it keeps its
     size and its scrollback. `inert`, so that it cannot hold the focus or be
     reached with Tab from there. -->
<div class="pane" class:hidden={!active} data-terminal inert={!active}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="term" bind:this={host} oncontextmenucapture={onContextMenu}></div>

  {#if finding}
    <div class="find" class:missing>
      <input
        type="text"
        bind:this={field}
        bind:value={wanted}
        placeholder="Find in terminal"
        aria-label="Find in terminal"
        spellcheck="false"
        oninput={() => (missing = false)}
        onkeydown={onFindKey}
      />
      <button title="Previous (Shift+Enter)" aria-label="Previous match" onclick={() => lookFor(true)}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M2 6.5 L5 3.5 L8 6.5" fill="none" stroke="currentColor" stroke-width="1.3" />
        </svg>
      </button>
      <button title="Next (Enter)" aria-label="Next match" onclick={() => lookFor(false)}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M2 3.5 L5 6.5 L8 3.5" fill="none" stroke="currentColor" stroke-width="1.3" />
        </svg>
      </button>
      <button
        title="Close (Escape)"
        aria-label="Close find"
        onclick={() => {
          finding = false;
          handle?.focus();
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
    </div>
  {/if}
</div>

<style>
  .pane {
    position: absolute;
    inset: 0;
  }
  .hidden {
    visibility: hidden;
  }

  /* xterm measures this element to decide how many rows and columns fit, so
     the breathing room is around it, not padding inside it. */
  .term {
    position: absolute;
    inset: 6px 0 4px 10px;
  }

  /* The field for searching the output, in the corner where it covers the
     least: output is read from the left. */
  .find {
    position: absolute;
    top: 6px;
    right: 14px;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 5px;
    box-shadow: 0 6px 18px #0008;
  }
  .find input {
    width: 13rem;
    padding: 0.2rem 0.4rem;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 3px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.78rem;
  }
  .find input:focus-visible {
    outline: none;
    border-color: var(--accent);
  }
  /* Looked for, and not there. */
  .find.missing input {
    border-color: var(--danger);
  }
  .find button {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg-dim);
    cursor: pointer;
  }
  .find button:hover {
    background: var(--hover);
    color: var(--fg);
  }

  /* xterm ships an opaque black sheet behind its rows, which no theme setting
     reaches. The rows paint what needs painting; this would only cover the
     stage, and make a terminal the one part of the window that ignores the
     opacity setting. */
  .term :global(.xterm-viewport) {
    background-color: transparent !important;
  }

  /* The cursor fades out and back rather than switching off and on. xterm
     blinks it with keyframes of its own, stepped; the same keyframes eased
     are a fade for the block and the bar. The underline's hide a border by
     its style, which has nothing between there and gone, so it is given a
     set that fades the colour instead. */
  .term :global(.xterm-cursor.xterm-cursor-blink) {
    animation-duration: 1.25s !important;
    animation-timing-function: ease-in-out !important;
  }
  .term :global(.xterm-cursor.xterm-cursor-blink.xterm-cursor-underline) {
    animation-name: thread-underline-fade !important;
  }
  @keyframes -global-thread-underline-fade {
    50% {
      border-bottom-color: transparent;
    }
  }
</style>
