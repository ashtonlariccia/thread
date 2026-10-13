<script lang="ts">
  /**
   * The stage, while the window is showing tmux: the panes of the window
   * in front, each where tmux has put it, under a strip that names the
   * window and holds the buttons for its panes.
   *
   * tmux lays a window out in character cells, for the room it is told there
   * is. So this measures a cell, says how many fit, and draws each pane at
   * the cells tmux gives it. The one cell tmux leaves between two panes, for
   * the line it would draw, is where the line that drags them is.
   */
  import { onMount, untrack } from "svelte";

  import type { Link } from "../links";
  import type { RemoteStatus } from "../state/remote.svelte";
  import type { TerminalLook, TerminalRenderer } from "../terminal";
  import type { CellSize, Probe } from "./pane";
  import TmuxPaneView from "./TmuxPaneView.svelte";
  import TmuxHeader from "./TmuxHeader.svelte";
  import type { Tmux, TmuxDivider, TmuxPane } from "./tmux.svelte";

  type Props = {
    tmux: Tmux;
    /** Whether this is the face the window is showing. */
    shown: boolean;
    /** Something is up over the window, and the keyboard is its. */
    blocked: boolean;
    /** The sidebar is folded away, and the card has its own left edge to keep. */
    railed: boolean;
    remoteStatus: RemoteStatus;
    /** The remote's name, to say whose sessions these are. */
    host: string;
    look: TerminalLook;
    scrollback: number;
    renderer: TerminalRenderer;
    /** Ask who to connect to: tmux is a remote's. */
    onconnect: () => void;
    onlink: (link: Link) => void;
    /** Close a pane, which may be asked about first. */
    onclosepane: (pane: TmuxPane) => void;
  };

  let {
    tmux,
    shown,
    blocked,
    railed,
    remoteStatus,
    host,
    look,
    scrollback,
    renderer,
    onconnect,
    onlink,
    onclosepane,
  }: Props = $props();

  // --- the size of things -----------------------------------------------------------

  let area = $state<HTMLElement | undefined>();
  let probeHost = $state<HTMLElement | undefined>();
  let probe: Probe | null = null;
  /** One character cell, in CSS pixels; null until it has been measured. */
  let cell = $state.raw<CellSize | null>(null);

  let sizing: ReturnType<typeof setTimeout> | undefined;

  /** Measure a cell and the room, and tell tmux how many cells there are. */
  function measure() {
    // Nothing can be measured while it is not laid out.
    if (!area || area.clientWidth === 0 || area.clientHeight === 0) return;
    const found = probe?.cell() ?? null;
    if (!found) return;
    if (found.w !== cell?.w || found.h !== cell?.h) cell = found;

    const style = getComputedStyle(area);
    const width = area.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const height = area.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    tmux.resize(Math.floor(width / found.w), Math.floor(height / found.h));
  }

  /**
   * Debounced: a drag of the window's edge reports every pixel, and each new
   * size is every program in every pane redrawing itself.
   */
  function remeasure() {
    clearTimeout(sizing);
    sizing = setTimeout(measure, 80);
  }

  onMount(() => {
    let gone = false;

    void import("./pane").then(({ openProbe }) => {
      if (gone || !probeHost) return;
      probe = openProbe(probeHost, look, renderer);
      // Once it has been drawn, which is when it knows its own measure.
      requestAnimationFrame(measure);
      // A font that arrives late changes what a cell is.
      void document.fonts.ready.then(() => !gone && remeasure());
    });

    const observer = new ResizeObserver(remeasure);
    if (area) observer.observe(area);

    return () => {
      gone = true;
      clearTimeout(sizing);
      observer.disconnect();
      probe?.dispose();
      probe = null;
    };
  });

  // The font or its size changing is every cell changing.
  $effect(() => {
    const next = look;
    untrack(() => {
      probe?.restyle(next);
      for (const handle of tmux.handles) handle.restyle(next);
      remeasure();
    });
  });

  // Coming back to this face: the window may have changed size meanwhile.
  $effect(() => {
    if (shown) untrack(remeasure);
  });

  // --- the panes --------------------------------------------------------------------

  /**
   * The windows that have been in front. Each keeps its panes' terminals
   * from then on, so going back to one is instant; a window never looked at
   * costs nothing, and tmux is not asked to send what it prints.
   */
  let visited = $state.raw<number[]>([]);
  $effect(() => {
    const windows = tmux.state?.windows ?? [];
    const front = tmux.window?.id ?? null;
    untrack(() => {
      const next = visited.filter((id) => windows.some((each) => each.id === id));
      if (front !== null && !next.includes(front)) next.push(front);
      if (next.length !== visited.length || next.some((id, i) => id !== visited[i])) visited = next;
    });
  });

  const front = $derived(tmux.window);
  const mounted = $derived(
    (tmux.state?.windows ?? [])
      .filter((window) => window.id === front?.id || visited.includes(window.id))
      .flatMap((window) => window.panes.map((pane) => ({ pane, window: window.id }))),
  );

  /** Where a box of cells goes, as CSS. */
  function place(box: { x: number; y: number; w: number; h: number }, size: CellSize): string {
    return (
      `left:${box.x * size.w}px;top:${box.y * size.h}px;` +
      `width:${box.w * size.w}px;height:${box.h * size.h}px`
    );
  }

  /** Counts the terminals that have come up, for what waits on one to be there. */
  let ready = $state(0);

  // The keyboard goes to the pane tmux says is typed into: on coming to
  // this face, on a new pane, and on tmux moving it.
  $effect(() => {
    void ready;
    const pane = tmux.pane?.id;
    if (!shown || blocked || pane === undefined) return;
    untrack(() => tmux.handle(pane)?.focus());
  });

  /** A pane was clicked or tabbed into: that is the one to type into. */
  function took(pane: TmuxPane) {
    if (!pane.active) tmux.act({ do: "selectPane", pane: pane.id });
  }

  // --- the lines between panes ---------------------------------------------------------

  /** The line being dragged, and how far it has been, in cells. */
  let dragging = $state<{ line: TmuxDivider; from: number; delta: number } | null>(null);

  function startDrag(event: PointerEvent, line: TmuxDivider) {
    if (event.button !== 0) return;
    dragging = { line, from: line.dir === "row" ? event.clientX : event.clientY, delta: 0 };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function drag(event: PointerEvent) {
    if (!dragging || !cell) return;
    const { line, from } = dragging;
    const moved =
      line.dir === "row" ? (event.clientX - from) / cell.w : (event.clientY - from) / cell.h;
    dragging.delta = Math.max(line.lo, Math.min(line.hi, Math.round(moved)));
  }

  /**
   * Let go: tmux is told once, here, and not at every cell on the way. Each
   * telling is a new size for the programs either side to redraw at.
   */
  function endDrag() {
    const done = dragging;
    dragging = null;
    if (!done || done.delta === 0 || !front) return;
    tmux.act({ do: "moveDivider", window: front.id, divider: done.line.id, delta: done.delta });
  }

  function placeLine(line: TmuxDivider, size: CellSize): string {
    const moved = dragging?.line.id === line.id ? dragging.delta : 0;
    // Down the middle of the cell tmux leaves between the two.
    return line.dir === "row"
      ? `left:${(line.x + 0.5 + moved) * size.w}px;top:${line.y * size.h}px;--length:${line.length * size.h}px`
      : `left:${line.x * size.w}px;top:${(line.y + 0.5 + moved) * size.h}px;--length:${line.length * size.w}px`;
  }
</script>

<section class="stage" class:railed>
  {#if tmux.status === "attached" && tmux.state}
    <TmuxHeader
      window={front}
      command={tmux.pane?.command ?? ""}
      onsplit={(dir) => tmux.pane && tmux.act({ do: "split", pane: tmux.pane.id, dir })}
      onzoom={() => tmux.pane && tmux.act({ do: "zoom", pane: tmux.pane.id })}
      onclosepane={() => tmux.pane && onclosepane(tmux.pane)}
    />
  {:else}
    <!-- The strip's room, kept: the room under it is what tmux is told
         about before there is a strip to show. -->
    <div class="no-tabs"></div>
  {/if}

  <div class="area" bind:this={area}>
    <!-- Never seen: it is there to be measured (see `openProbe`). -->
    <div class="probe" bind:this={probeHost} aria-hidden="true" inert></div>

    {#if tmux.status === "attached" && cell}
      {@const size = cell}
      <div class="grid">
        {#each mounted as { pane, window } (pane.id)}
          {@const visible = window === front?.id}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="pane"
            class:behind={!visible}
            class:active={visible && pane.active && (front?.panes.length ?? 0) > 1}
            data-terminal
            inert={!visible}
            style={place(pane, size)}
            onfocusin={() => took(pane)}
            onpointerdowncapture={() => took(pane)}
          >
            <TmuxPaneView
              {tmux}
              pane={pane.id}
              cols={pane.w}
              rows={pane.h}
              {look}
              {scrollback}
              {renderer}
              onready={() => ready++}
              {onlink}
            />
          </div>
        {/each}

        {#each front?.dividers ?? [] as line (line.id)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            class="divider"
            class:across={line.dir === "column"}
            class:dragging={dragging?.line.id === line.id}
            role="separator"
            aria-orientation={line.dir === "row" ? "vertical" : "horizontal"}
            style={placeLine(line, size)}
            onpointerdown={(event) => startDrag(event, line)}
            onpointermove={drag}
            onpointerup={endDrag}
            onpointercancel={() => (dragging = null)}
          ></div>
        {/each}
      </div>
    {:else}
      <!-- Not attached to anything: say why, and what there is to do about it. -->
      <div class="notice">
        {#if remoteStatus === "local"}
          <p>tmux sessions live on a remote. Connect to one to see its sessions.</p>
          <button class="primary" onclick={onconnect}>Connect…</button>
        {:else if remoteStatus === "connecting"}
          <p>Connecting…</p>
        {:else if remoteStatus === "lost" || tmux.status === "offline"}
          <p>The connection to {host} has dropped.</p>
          <p class="dim">Your sessions are still running there. Reconnecting…</p>
        {:else if tmux.status === "missing"}
          <p>There is no tmux on {host}.</p>
          <p class="dim">Install it there, then switch back to this view.</p>
          <button onclick={() => void tmux.refresh()}>Look Again</button>
        {:else if tmux.status === "detached"}
          {#if tmux.problem}
            <p>{tmux.problem}</p>
          {:else if tmux.listing.length === 0}
            <p>No tmux sessions on {host}.</p>
          {:else}
            <p>Not attached to a session.</p>
          {/if}
          <p class="dim">
            A session keeps running on {host} when this window closes, or the connection drops.
          </p>
          <button class="primary" onclick={() => tmux.create("")}>New Session</button>
        {:else}
          <p class="dim">Attaching…</p>
        {/if}
      </div>
    {/if}
  </div>
</section>

<style>
  /* The same card the editor's panes sit in (`.stage` in `App`). */
  .stage {
    --tabs-height: 26px;

    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    margin: var(--viewport-inset) var(--viewport-inset) var(--viewport-inset) 0;
    background: var(--bg-viewport-wash);
    border-radius: var(--viewport-radius);
    transition: margin-left 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
    overflow: hidden;
  }
  .stage.railed {
    margin-left: var(--viewport-inset);
  }
  .no-tabs {
    flex: none;
    height: var(--tabs-height);
  }

  @media (prefers-reduced-motion: reduce) {
    .stage {
      transition: none;
    }
  }

  /* The room tmux is told about is this, less its padding: enough of an
     edge that text does not touch the card's, and that the ring round the
     pane typed into is not cut off. */
  .area {
    position: relative;
    flex: 1;
    min-height: 0;
    padding: 6px 6px 4px 8px;
    overflow: hidden;
  }

  .probe {
    position: absolute;
    left: 0;
    top: 0;
    width: 200px;
    height: 60px;
    visibility: hidden;
    pointer-events: none;
  }

  /* What the panes and the lines between them are placed in, cell by cell. */
  .grid {
    position: relative;
    width: 100%;
    height: 100%;
  }

  .pane {
    position: absolute;
    overflow: hidden;
  }
  /* A pane of a window that is not in front keeps its terminal, and takes
     no click and no key meanwhile. */
  .pane.behind {
    visibility: hidden;
    pointer-events: none;
  }
  /* The pane the keyboard is in, where there is more than one: tmux says it
     with the colour of the lines round the pane, and so does this. */
  .pane.active {
    overflow: visible;
  }
  .pane.active::after {
    content: "";
    position: absolute;
    inset: -3px;
    border: 1px solid color-mix(in srgb, var(--accent) 55%, transparent);
    border-radius: 4px;
    pointer-events: none;
  }

  /* The line between two panes, as between two of the editor's (`App`),
     here a length in pixels: a whole number of cells. */
  .divider {
    position: absolute;
    z-index: 5;
    width: 7px;
    height: var(--length);
    transform: translateX(-50%);
    cursor: col-resize;
  }
  .divider.across {
    width: var(--length);
    height: 7px;
    transform: translateY(-50%);
    cursor: row-resize;
  }
  .divider::after {
    content: "";
    position: absolute;
    inset: 0 3px;
    background: var(--border);
    transition: background 120ms ease;
  }
  .divider.across::after {
    inset: 3px 0;
  }
  .divider:hover::after,
  .divider.dragging::after {
    background: var(--accent);
  }

  .notice {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    padding: 1.5rem;
    text-align: center;
    font-size: 0.86rem;
    user-select: none;
  }
  .notice p {
    margin: 0;
    max-width: 30rem;
  }
  .notice .dim {
    color: var(--fg-dim);
    font-size: 0.8rem;
  }
  .notice button {
    margin-top: 0.6rem;
    padding: 0.35rem 0.9rem;
    background: var(--hover);
    border: 1px solid var(--border-input);
    border-radius: 5px;
    color: var(--fg);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }
  .notice button:hover {
    border-color: var(--accent);
  }
  .notice button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-ink);
  }
</style>
