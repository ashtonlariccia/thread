/**
 * This window's tmux: the sessions on the remote it is connected to, and
 * the one it is attached to.
 *
 * The backend holds the attachment and speaks tmux's control mode
 * (`tmux.rs`). What arrives here is already what there is to draw: the
 * sessions, this one's windows with each pane laid out in cells, and what
 * each pane with a terminal on the page has printed. Everything asked of
 * tmux goes back as an action, and shows once tmux says it has happened:
 * nothing here keeps an account of its own for tmux's to drift from.
 */
import { Channel, invoke } from "@tauri-apps/api/core";

import type { PaneHandle } from "./pane";

export type Dir = "row" | "column";

/** A pane, as what runs in it is found: its first process, and its terminal. */
export type PaneBrief = { id: number; pid: number; tty: string };

/** A window of any session, as the list of sessions shows it. */
export type WindowBrief = {
  id: number;
  index: number;
  name: string;
  /** The window of its session that is in front. */
  active: boolean;
  panes: PaneBrief[];
};

export type SessionInfo = {
  id: number;
  name: string;
  windows: number;
  attached: number;
  /** Its windows, in order. Empty while not attached to tmux: there is no one to ask. */
  list: WindowBrief[];
};

/** A process on one of the remote's terminals. Mirrors `Process` in the core. */
export type Process = {
  pid: number;
  ppid: number;
  /** As `ps` names it: `pts/3`. */
  tty: string;
  /** Percent of one core. */
  cpu: number;
  command: string;
};

/** Something run in a pane, with everything it has started counted into it. */
export type Job = {
  pid: number;
  /** The pane it was run in. */
  pane: number;
  /** Its command line, the folders of the program's own path left off. */
  command: string;
  /** Percent of one core, its children's included. */
  cpu: number;
  /** How many processes it has started, and they have. */
  children: number;
};

export type TmuxPane = {
  id: number;
  x: number;
  y: number;
  w: number;
  h: number;
  /** The pane of its window that is typed into. */
  active: boolean;
  /** What is running in it, as tmux names it. */
  command: string;
};

/** A line between two panes, and how far it can be dragged, in cells. */
export type TmuxDivider = {
  id: string;
  dir: Dir;
  x: number;
  y: number;
  length: number;
  lo: number;
  hi: number;
};

export type TmuxWindow = {
  id: number;
  index: number;
  name: string;
  active: boolean;
  zoomed: boolean;
  cols: number;
  rows: number;
  panes: TmuxPane[];
  paneCount: number;
  dividers: TmuxDivider[];
};

export type TmuxState = {
  session: number | null;
  sessions: SessionInfo[];
  windows: TmuxWindow[];
};

/** Something asked of tmux. Mirrors `Action` in the core. */
export type Action =
  | { do: "resize"; cols: number; rows: number }
  | { do: "watch" | "unwatch" | "selectPane" | "killPane" | "zoom"; pane: number }
  | { do: "write"; pane: number; text: string }
  | { do: "writeBytes"; pane: number; bytes: number[] }
  | { do: "split"; pane: number; dir: Dir }
  | { do: "selectWindow"; window: number; session?: number }
  | { do: "killWindow"; window: number }
  | { do: "newWindow"; session?: number }
  | { do: "renameWindow"; window: number; name: string }
  | { do: "switchSession" | "killSession"; session: number }
  | { do: "newSession"; name: string }
  | { do: "renameSession"; session: number; name: string }
  | { do: "moveDivider"; window: number; divider: string; delta: number };

type Message =
  | { type: "state"; state: TmuxState }
  /** Each pane with a new size, its columns and its rows. */
  | { type: "sizes"; panes: [number, number, number][] }
  | { type: "error"; message: string }
  | { type: "exit"; ended: { attached: boolean; reason: string | null } };

export type TmuxStatus =
  /** Not looked at yet, or the window is on this machine. */
  | "idle"
  /** Asking the remote what sessions it has. */
  | "listing"
  /** The remote has no tmux to run. */
  | "missing"
  /** Not attached: there is no session, or the one tried would not have us. */
  | "detached"
  | "attaching"
  | "attached"
  /** The connection has dropped. The sessions are still there, on the remote. */
  | "offline";

/**
 * How much output is drawn between one word to the backend that it has been
 * and the next: see `ACK_EVERY` in `terminal.ts`, which this is.
 */
const ACK_EVERY = 32 * 1024;

/** The names a shell goes by: a pane running one has nothing to lose. */
const SHELL = /^-?(ba|z|fi|da|a|k|c|tc)?sh$|^(nu|xonsh|elvish|pwsh|login)$/;

/** The program a command line runs, by its own name: `/usr/bin/make -j8` is `make`. */
function program(command: string): string {
  const first = command.split(" ", 1)[0];
  return first.slice(first.lastIndexOf("/") + 1);
}

/**
 * What is running in a window: for each of its panes, what was run from
 * the pane's shell, each with what it has started in turn counted into it.
 * A build is then one line, however many compilers it has going. A pane
 * that was started on a program rather than a shell is that program.
 */
export function jobs(panes: PaneBrief[], processes: Process[]): Job[] {
  const started = new Map<number, Process[]>();
  for (const process of processes) {
    const siblings = started.get(process.ppid);
    if (siblings) siblings.push(process);
    else started.set(process.ppid, [process]);
  }
  const tally = (process: Process): { cpu: number; children: number } => {
    let cpu = process.cpu;
    let children = 0;
    for (const child of started.get(process.pid) ?? []) {
      const below = tally(child);
      cpu += below.cpu;
      children += 1 + below.children;
    }
    return { cpu, children };
  };

  const found: Job[] = [];
  for (const pane of panes) {
    const tty = pane.tty.replace(/^\/dev\//, "");
    let tops = (started.get(pane.pid) ?? []).filter((process) => process.tty === tty);
    if (tops.length === 0) {
      const root = processes.find((process) => process.pid === pane.pid);
      if (root && !SHELL.test(program(root.command))) tops = [root];
    }
    for (const top of tops) {
      const name = program(top.command);
      const rest = top.command.slice(top.command.split(" ", 1)[0].length);
      found.push({ pid: top.pid, pane: pane.id, command: name + rest, ...tally(top) });
    }
  }
  return found;
}

/** Whether a pane is running something other than a shell waiting to be typed at. */
export function busy(pane: TmuxPane): boolean {
  return pane.command !== "" && !SHELL.test(pane.command);
}

export class Tmux {
  status = $state<TmuxStatus>("idle");
  /** What there is, while attached. */
  state = $state.raw<TmuxState | null>(null);
  /** The sessions there were when last asked, for while not attached. */
  listing = $state.raw<SessionInfo[]>([]);
  /** Why the last attempt to attach came to nothing, or what tmux last refused. */
  problem = $state<string | null>(null);
  /**
   * The window in front. Set the moment a tab is clicked, without waiting
   * to hear back; what tmux says is in front replaces it when it does.
   */
  front = $state<number | null>(null);

  /** tmux has refused something that was asked for. */
  onerror: (message: string) => void = () => {};

  /** Whether the window has been shown its tmux face at all. */
  #entered = false;
  #online = false;
  /** The room there is for a window, in cells; null until it is measured. */
  #size: { cols: number; rows: number } | null = null;
  /** What to attach to once there is a size to attach with. */
  #waiting: { session: number | null; name: string } | null = null;
  /** The session last attached to, to come back to after the connection drops. */
  #wanted: number | null = null;
  /** Counts attachments, so what a finished one says late is not heard. */
  #generation = 0;
  #handles = new Map<number, PaneHandle>();
  #drawn = 0;

  /** The sessions to list: tmux's own account while attached. */
  get sessions(): SessionInfo[] {
    return this.state?.sessions ?? this.listing;
  }

  get session(): SessionInfo | null {
    const id = this.state?.session ?? null;
    return this.state?.sessions.find((each) => each.id === id) ?? null;
  }

  get window(): TmuxWindow | null {
    const windows = this.state?.windows ?? [];
    return windows.find((each) => each.id === this.front) ?? windows.find((each) => each.active) ?? null;
  }

  /** The pane of the window in front that is typed into. */
  get pane(): TmuxPane | null {
    const panes = this.window?.panes ?? [];
    return panes.find((each) => each.active) ?? panes[0] ?? null;
  }

  // --- coming and going -----------------------------------------------------------

  /** The window has turned to its tmux face. */
  enter() {
    this.#entered = true;
    if (this.#online && this.status === "idle") void this.refresh();
  }

  /**
   * The window's connection has changed: up, dropped, or left on purpose.
   * Dropped, what was attached is come back to when it is up again; left,
   * it is forgotten.
   */
  link(status: "local" | "connecting" | "connected" | "lost") {
    if (status === "connecting") return;
    this.#online = status === "connected";
    if (status === "local") {
      this.#detach();
      this.#wanted = null;
      this.listing = [];
      this.problem = null;
      this.status = "idle";
    } else if (status === "lost") {
      if (this.status !== "idle") this.status = "offline";
    } else if (this.#entered && (this.status === "idle" || this.status === "offline")) {
      void this.refresh();
    }
  }

  /** Ask what sessions there are, and attach to one if there is one. */
  async refresh() {
    if (!this.#online) return;
    const generation = ++this.#generation;
    this.status = "listing";
    let listing: { installed: boolean; sessions: SessionInfo[] };
    try {
      listing = await invoke("tmux_sessions");
    } catch (e) {
      if (generation !== this.#generation) return;
      this.problem = String(e);
      this.status = this.#online ? "detached" : "offline";
      return;
    }
    if (generation !== this.#generation) return;
    this.problem = null;
    this.listing = listing.sessions;
    if (!listing.installed) {
      this.status = "missing";
      return;
    }
    // Back to the one it was on, if that is still there; else the first.
    const back = listing.sessions.find((each) => each.id === this.#wanted) ?? listing.sessions[0];
    if (back) this.attach(back.id);
    else this.status = "detached";
  }

  /** Attach to a session that is there. */
  attach(session: number) {
    this.#start({ session, name: "" });
  }

  /** Make a session and attach to it. With no name, tmux numbers it. */
  create(name: string) {
    if (this.status === "attached") this.act({ do: "newSession", name });
    else this.#start({ session: null, name });
  }

  /** Go to another session, attached or not. */
  select(session: number) {
    if (this.status === "attached") {
      if (session !== this.state?.session) this.act({ do: "switchSession", session });
    } else this.attach(session);
  }

  #start(what: { session: number | null; name: string }) {
    if (!this.#online) return;
    this.problem = null;
    // tmux lays a window out for the room it is told there is.
    if (!this.#size) {
      this.#waiting = what;
      this.status = "attaching";
      return;
    }
    this.#waiting = null;
    const generation = ++this.#generation;
    this.status = "attaching";

    const events = new Channel<ArrayBuffer | Message>();
    events.onmessage = (message) => {
      if (generation !== this.#generation) {
        // Still counted: the backend is waiting to hear it was drawn.
        if (message instanceof ArrayBuffer) this.#ack(message.byteLength);
        return;
      }
      if (message instanceof ArrayBuffer) this.#output(message);
      else if (message.type === "state") this.#arrived(message.state);
      // Ahead of the full account of it: what a pane prints next was
      // printed for this size, and is to be drawn at it.
      else if (message.type === "sizes") {
        for (const [pane, cols, rows] of message.panes) this.#handles.get(pane)?.resize(cols, rows);
      }
      else if (message.type === "error") this.onerror(message.message);
      else this.#ended(message.ended);
    };
    invoke("tmux_open", {
      onEvent: events,
      session: what.session,
      name: what.name,
      cols: this.#size.cols,
      rows: this.#size.rows,
    }).catch((e) => {
      if (generation !== this.#generation) return;
      this.problem = String(e);
      this.status = this.#online ? "detached" : "offline";
    });
  }

  #arrived(state: TmuxState) {
    const first = this.status !== "attached";
    this.state = state;
    this.status = "attached";
    this.problem = null;
    // The room may have changed while it was attaching, with no one to tell.
    if (first && this.#size) this.act({ do: "resize", ...this.#size });
    this.#wanted = state.session;
    this.front = state.windows.find((each) => each.active)?.id ?? null;
    // At once, and not when the page next draws: what a pane prints after
    // this was printed for the size it now has.
    for (const window of state.windows) {
      for (const pane of window.panes) this.#handles.get(pane.id)?.resize(pane.w, pane.h);
    }
  }

  #ended(ended: { attached: boolean; reason: string | null }) {
    this.state = null;
    this.front = null;
    if (!this.#online) {
      this.status = "offline";
      return;
    }
    if (ended.attached) {
      // Let go of by tmux: the session ended, or was detached from
      // elsewhere. If there is another, that one; if not, say so.
      void this.refresh();
      return;
    }
    // It never attached. Trying again would only fail the same way.
    this.problem = ended.reason;
    this.#wanted = null;
    this.status = "detached";
    void invoke<{ installed: boolean; sessions: SessionInfo[] }>("tmux_sessions").then(
      (listing) => {
        this.listing = listing.sessions;
        if (!listing.installed) this.status = "missing";
      },
      () => {},
    );
  }

  #detach() {
    this.#generation++;
    this.#waiting = null;
    this.state = null;
    this.front = null;
    void invoke("tmux_close").catch(() => {});
  }

  // --- panes --------------------------------------------------------------------

  /** A terminal has been made for a pane: have it shown, and kept current. */
  register(pane: number, handle: PaneHandle) {
    this.#handles.set(pane, handle);
    this.act({ do: "watch", pane });
  }

  /** A pane's terminal has gone: nothing it prints needs sending. */
  unregister(pane: number, handle: PaneHandle) {
    if (this.#handles.get(pane) !== handle) return;
    this.#handles.delete(pane);
    if (this.status === "attached") this.act({ do: "unwatch", pane });
  }

  handle(pane: number): PaneHandle | undefined {
    return this.#handles.get(pane);
  }

  /** Every pane's terminal, for what is done to them all. */
  get handles(): Iterable<PaneHandle> {
    return this.#handles.values();
  }

  /** What a pane printed: its number in the first four bytes, then that. */
  #output(message: ArrayBuffer) {
    const size = message.byteLength - 4;
    const handle = size > 0 ? this.#handles.get(new DataView(message).getUint32(0, true)) : undefined;
    if (!handle) {
      this.#ack(Math.max(size, 0));
      return;
    }
    handle.write(new Uint8Array(message, 4), () => this.#ack(size));
  }

  #ack(bytes: number) {
    this.#drawn += bytes;
    if (this.#drawn < ACK_EVERY) return;
    void invoke("tmux_drawn", { bytes: this.#drawn });
    this.#drawn = 0;
  }

  // --- asking -------------------------------------------------------------------

  act(action: Action) {
    if (this.status !== "attached") return;
    void invoke("tmux_do", { action });
  }

  /** The room there is for a window has been measured, in cells. */
  resize(cols: number, rows: number) {
    if (cols < 2 || rows < 2) return;
    if (this.#size?.cols === cols && this.#size.rows === rows) return;
    this.#size = { cols, rows };
    if (this.#waiting) this.#start(this.#waiting);
    else this.act({ do: "resize", cols, rows });
  }

  /** Bring a window to the front. */
  show(window: number, session = this.state?.session ?? undefined) {
    const here = session === (this.state?.session ?? undefined);
    if (here && this.front === window) return;
    if (here) this.front = window;
    this.act({ do: "selectWindow", window, session });
  }

  /** Step to the next or previous window, wrapping at the ends. */
  cycle(step: 1 | -1) {
    const windows = this.state?.windows ?? [];
    const at = windows.findIndex((each) => each.id === this.window?.id);
    if (windows.length < 2 || at === -1) return;
    this.show(windows[(at + step + windows.length) % windows.length].id);
  }
}
