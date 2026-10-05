<script module lang="ts">
  export const TERMINAL_MIN = 80;
</script>

<script lang="ts">
  /**
   * The terminal, in a panel that rises out of the bottom bar.
   *
   * Mounted for as long as there is a shell, shown or not: hiding the panel
   * folds it away with everything in it still running, and unmounting it is
   * what ends the shell. It has no header and no label — it is one terminal,
   * for wherever this window is working, and says so by being there.
   */
  import { onMount } from "svelte";

  import type { TerminalHandle, TerminalLook } from "./terminal";

  type Props = {
    /** Whether the panel is showing. The shell runs either way. */
    open: boolean;
    /** The height asked for; what is shown is this, kept within `max`. */
    height: number;
    /** The most the panel may take, so the editor above keeps some room. */
    max: number;
    /** Where the shell starts. Read once, when it does. */
    cwd: string | null;
    look: TerminalLook;
    scrollback: number;
    /** Reports a new height. The parent owns the value; this only asks. */
    onresize: (height: number) => void;
    /** The shell ended by itself. */
    onexit: () => void;
    oncontext: (event: MouseEvent) => void;
  };

  let { open, height, max, cwd, look, scrollback, onresize, onexit, oncontext }: Props = $props();

  let panel: HTMLElement;
  let host: HTMLElement;
  let handle = $state.raw<TerminalHandle | null>(null);
  /** False for the first frame, so a new panel rises rather than appears. */
  let revealed = $state(false);
  let dragging = $state(false);

  const clamp = (h: number) => Math.round(Math.max(TERMINAL_MIN, Math.min(h, max)));
  const shownHeight = $derived(clamp(height));
  const shown = $derived(open && revealed);

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
    const frame = requestAnimationFrame(() => (revealed = true));

    // Loaded here, the first time a terminal is wanted, rather than with the
    // window: see `terminal.ts`.
    void import("./terminal").then(({ openTerminal }) => {
      if (gone) return;
      handle = openTerminal(host, { cwd, ...look, scrollback, onexit });
    });

    return () => {
      gone = true;
      cancelAnimationFrame(frame);
      handle?.dispose();
    };
  });

  // Showing the panel is asking to type in it.
  $effect(() => {
    if (shown) handle?.focus();
  });

  $effect(() => {
    handle?.restyle(look);
  });

  // --- resizing ---------------------------------------------------------------

  function start(event: PointerEvent) {
    dragging = true;
    // Capture keeps the drag alive when the cursor outruns a 5px handle.
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function move(event: PointerEvent) {
    if (!dragging) return;
    // The panel's bottom edge stays put, so its height is how far above that
    // edge the pointer is.
    onresize(clamp(panel.getBoundingClientRect().bottom - event.clientY));
  }

  function end(event: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    const step = event.shiftKey ? 40 : 10;
    onresize(clamp(shownHeight + (event.key === "ArrowUp" ? step : -step)));
    event.preventDefault();
  }

  // In the capture phase, ahead of xterm: a program reading the mouse would
  // otherwise be sent the right-click as a click of its own.
  function onContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    oncontext(event);
  }
</script>

<!-- `inert` while folded away: a panel of no height must not hold the focus
     or be reachable with Tab. -->
<div
  class="panel"
  class:dragging
  data-terminal
  bind:this={panel}
  style:height="{shown ? shownHeight : 0}px"
  inert={!shown}
>
  <!-- Always its full height, and anchored to the panel's top edge: the panel
       opening is this sliding up from behind the bottom bar, and the terminal
       inside never has to be measured at a size it is only passing through. -->
  <div class="body" style:height="{shownHeight}px">
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="grip"
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize terminal"
      aria-valuenow={shownHeight}
      aria-valuemin={TERMINAL_MIN}
      aria-valuemax={max}
      tabindex="0"
      onpointerdown={start}
      onpointermove={move}
      onpointerup={end}
      onpointercancel={end}
      onkeydown={onKey}
    ></div>

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="term" bind:this={host} oncontextmenucapture={onContextMenu}></div>
  </div>
</div>

<style>
  .panel {
    position: relative;
    flex: none;
    overflow: hidden;
    transition: height 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
  }
  /* The height is following the pointer; easing towards it would lag it. */
  .panel.dragging {
    transition: none;
  }

  .body {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    /* The one line between the editor and the terminal. Neither paints a
       background: both sit on the stage's. */
    border-top: 1px solid var(--border);
  }

  /* Straddles the dividing line, so the line itself is what is taken hold
     of, and costs no space of its own. */
  .grip {
    position: absolute;
    top: -3px;
    left: 0;
    right: 0;
    height: 8px;
    z-index: 5;
    cursor: row-resize;
  }
  .grip::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 0;
    right: 0;
    height: 1px;
    background: transparent;
    transition: background 120ms ease;
  }
  .grip:hover::after,
  .grip:focus-visible::after,
  .dragging .grip::after {
    background: var(--accent);
  }
  .grip:focus-visible {
    outline: none;
  }

  /* xterm measures this element to decide how many rows and columns fit, so
     the breathing room is a margin on it, not padding inside it. */
  .term {
    flex: 1;
    min-width: 0;
    min-height: 0;
    margin: 6px 0 4px 10px;
  }

  /* xterm ships an opaque black sheet behind its rows, which no theme setting
     reaches. The rows paint what needs painting; this would only cover the
     stage, and make the terminal the one part of the window that ignores the
     opacity setting. */
  .term :global(.xterm-viewport) {
    background-color: transparent !important;
  }

  @media (prefers-reduced-motion: reduce) {
    .panel {
      transition: none;
    }
  }
</style>
