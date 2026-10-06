<script lang="ts">
  /**
   * One terminal, filling the space a file's text would.
   *
   * Mounted for as long as its tab is open, shown or not: switching to another
   * tab hides it with everything in it still running, and unmounting it is
   * what ends the shell.
   */
  import { onMount } from "svelte";

  import type { TerminalHandle, TerminalLook } from "./terminal";

  type Props = {
    /** Whether this is the tab being shown. The shell runs either way. */
    active: boolean;
    /** Whether it is being shown in the pane the keyboard is in. */
    focused: boolean;
    /** Where the shell starts. Read once, when it does. */
    cwd: string | null;
    look: TerminalLook;
    scrollback: number;
    /** The shell ended by itself. */
    onexit: () => void;
    /** The `thread` command was run in it, on a file or a folder. */
    onopen: (kind: "file" | "dir", path: string) => void;
    oncontext: (event: MouseEvent) => void;
  };

  let { active, focused, cwd, look, scrollback, onexit, onopen, oncontext }: Props = $props();

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
      handle = openTerminal(host, { cwd, ...look, scrollback, onexit, onopen });
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
