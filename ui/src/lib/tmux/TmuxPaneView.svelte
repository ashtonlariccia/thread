<script lang="ts">
  /**
   * One tmux pane's terminal, the size tmux says the pane is.
   *
   * Mounted for as long as the pane is in sight, or was while its window is
   * still open: unmounting it only stops the page hearing from the pane.
   * The pane itself, and what runs in it, are tmux's.
   */
  import { onMount } from "svelte";

  import type { Link } from "../links";
  import type { TerminalLook, TerminalRenderer } from "../terminal";
  import type { PaneHandle } from "./pane";
  import type { Tmux } from "./tmux.svelte";

  type Props = {
    tmux: Tmux;
    pane: number;
    /** The size tmux has given it, in cells. Read when its terminal is made. */
    cols: number;
    rows: number;
    look: TerminalLook;
    scrollback: number;
    /** What draws it. Read once. */
    renderer: TerminalRenderer;
    /** Its terminal is up, and can be given the keyboard. */
    onready: () => void;
    onlink: (link: Link) => void;
    oncontext: (event: MouseEvent) => void;
  };

  let { tmux, pane, cols, rows, look, scrollback, renderer, onready, onlink, oncontext }: Props =
    $props();

  let host: HTMLElement;
  let handle = $state.raw<PaneHandle | null>(null);

  onMount(() => {
    let gone = false;

    void import("./pane").then(({ openPane }) => {
      if (gone) return;
      handle = openPane(host, {
        ...look,
        renderer,
        scrollback,
        cols,
        rows,
        oninput: (text) => tmux.act({ do: "write", pane, text }),
        onbytes: (bytes) => tmux.act({ do: "writeBytes", pane, bytes }),
        onlink,
      });
      tmux.register(pane, handle);
      onready();
    });

    return () => {
      gone = true;
      if (handle) {
        tmux.unregister(pane, handle);
        handle.dispose();
      }
    };
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

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="term" bind:this={host} oncontextmenucapture={onContextMenu}></div>

<style>
  /* Exactly the pane: xterm draws the rows and columns it is told to, and
     the box around this is sized to hold them. */
  .term {
    position: absolute;
    inset: 0;
  }

  /* As a terminal in the editor's tabs: no black sheet behind the rows, and
     a cursor that fades (see `TerminalView`). */
  .term :global(.xterm-viewport) {
    background-color: transparent !important;
  }
  .term :global(.xterm-cursor.xterm-cursor-blink) {
    animation-duration: 1.25s !important;
    animation-timing-function: ease-in-out !important;
  }
  .term :global(.xterm-cursor.xterm-cursor-blink.xterm-cursor-underline) {
    animation-name: thread-underline-fade !important;
  }
</style>
