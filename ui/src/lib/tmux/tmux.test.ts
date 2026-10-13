import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend, stood in for: what was asked of it, and the channel an
// attachment was given to hear on.
type Call = { command: string; args: Record<string, unknown> };
const backend = vi.hoisted(() => ({
  calls: [] as { command: string; args: Record<string, unknown> }[],
  sessions: [] as { id: number; name: string; windows: number; attached: number }[],
  installed: true,
  channels: [] as { onmessage: (message: unknown) => void }[],
}));

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (message: unknown) => void = () => {};
    constructor() {
      backend.channels.push(this);
    }
  },
  invoke: async (command: string, args: Record<string, unknown> = {}) => {
    backend.calls.push({ command, args });
    if (command === "tmux_sessions") {
      return { installed: backend.installed, sessions: backend.sessions };
    }
    return undefined;
  },
}));

import type { PaneHandle } from "./pane";
import { busy, jobs, Tmux, type Process, type TmuxPane, type TmuxState } from "./tmux.svelte";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const asked = (command: string): Call[] => backend.calls.filter((call) => call.command === command);
const actions = () => asked("tmux_do").map((call) => call.args.action as { do: string });
const say = (message: unknown) => backend.channels.at(-1)!.onmessage(message);

const pane = (id: number, more: Partial<TmuxPane> = {}): TmuxPane => ({
  id,
  x: 0,
  y: 0,
  w: 100,
  h: 30,
  active: true,
  command: "bash",
  ...more,
});

function state(session: number, panes: TmuxPane[] = [pane(0)]): TmuxState {
  return {
    session,
    sessions: backend.sessions.map((each) => ({ ...each, list: [] })),
    windows: [
      {
        id: 7,
        index: 0,
        name: "bash",
        active: true,
        zoomed: false,
        cols: 100,
        rows: 30,
        panes,
        paneCount: panes.length,
        dividers: [],
      },
    ],
  };
}

/** A terminal that keeps what it is given, and says at once that it drew it. */
function handle() {
  const written: number[] = [];
  const sizes: [number, number][] = [];
  const made = {
    write: (bytes: Uint8Array, done: () => void) => {
      written.push(...bytes);
      done();
    },
    resize: (cols: number, rows: number) => sizes.push([cols, rows]),
  } as unknown as PaneHandle;
  return { made, written, sizes };
}

function output(to: number, bytes: number[]): ArrayBuffer {
  const message = new Uint8Array(4 + bytes.length);
  new DataView(message.buffer).setUint32(0, to, true);
  message.set(bytes, 4);
  return message.buffer;
}

/** A store on a connected window, turned to and measured, with what it attached to. */
async function attached(): Promise<Tmux> {
  const tmux = new Tmux();
  tmux.link("connected");
  tmux.enter();
  tmux.resize(100, 30);
  await settle();
  say({ type: "state", state: state(backend.sessions[0].id) });
  return tmux;
}

beforeEach(() => {
  backend.calls.length = 0;
  backend.channels.length = 0;
  backend.installed = true;
  backend.sessions = [{ id: 3, name: "main", windows: 1, attached: 0 }];
});

describe("the window's tmux", () => {
  it("does nothing until it is turned to, and then attaches to what is there", async () => {
    const tmux = new Tmux();
    tmux.link("connected");
    await settle();
    expect(backend.calls.filter((call) => call.command !== "tmux_close")).toEqual([]);

    tmux.enter();
    await settle();
    // There is a session, but no room has been measured to attach with.
    expect(tmux.status).toBe("attaching");
    expect(asked("tmux_open")).toEqual([]);

    tmux.resize(120, 40);
    expect(asked("tmux_open")[0].args).toMatchObject({ session: 3, cols: 120, rows: 40 });

    say({ type: "state", state: state(3) });
    expect(tmux.status).toBe("attached");
    expect(tmux.session?.name).toBe("main");
    expect(tmux.window?.id).toBe(7);
    expect(tmux.pane?.id).toBe(0);
  });

  it("says so when there is no session, and starts one when asked", async () => {
    backend.sessions = [];
    const tmux = new Tmux();
    tmux.link("connected");
    tmux.enter();
    tmux.resize(100, 30);
    await settle();
    expect(tmux.status).toBe("detached");
    expect(asked("tmux_open")).toEqual([]);

    tmux.create("build");
    expect(asked("tmux_open")[0].args).toMatchObject({ session: null, name: "build" });
  });

  it("says so when the remote has no tmux", async () => {
    backend.installed = false;
    backend.sessions = [];
    const tmux = new Tmux();
    tmux.link("connected");
    tmux.enter();
    await settle();
    expect(tmux.status).toBe("missing");
  });

  it("gives a pane's output to its terminal, and says when it is drawn", async () => {
    const tmux = await attached();
    const first = handle();
    tmux.register(0, first.made);
    expect(actions().at(-1)).toEqual({ do: "watch", pane: 0 });

    say(output(0, [104, 105]));
    expect(first.written).toEqual([104, 105]);
    expect(asked("tmux_drawn")).toEqual([]);

    // A pane with no terminal is counted as drawn all the same, or the
    // backend would wait for ever to hear that it was.
    say(output(9, new Array(40_000).fill(120)));
    expect(asked("tmux_drawn")[0].args.bytes).toBe(40_002);
  });

  it("gives each terminal the size tmux has given its pane", async () => {
    const tmux = await attached();
    const first = handle();
    tmux.register(0, first.made);
    say({ type: "state", state: state(3, [pane(0, { w: 49, h: 30 }), pane(1, { x: 50, w: 50 })]) });
    expect(first.sizes.at(-1)).toEqual([49, 30]);

    // And sooner than that, when tmux says so ahead of the rest.
    say({ type: "sizes", panes: [[0, 49, 14]] });
    expect(first.sizes.at(-1)).toEqual([49, 14]);
  });

  it("goes to another session when its own ends, and says so when none is left", async () => {
    backend.sessions = [
      { id: 3, name: "main", windows: 1, attached: 1 },
      { id: 5, name: "build", windows: 2, attached: 0 },
    ];
    const tmux = await attached();
    expect(tmux.state?.session).toBe(3);

    backend.sessions = [{ id: 5, name: "build", windows: 2, attached: 0 }];
    say({ type: "exit", ended: { attached: true, reason: null } });
    await settle();
    expect(asked("tmux_open").at(-1)!.args).toMatchObject({ session: 5 });

    backend.sessions = [];
    say({ type: "exit", ended: { attached: true, reason: null } });
    await settle();
    expect(tmux.status).toBe("detached");
    expect(tmux.state).toBeNull();
  });

  it("does not try again what never attached", async () => {
    const tmux = await attached();
    const opened = asked("tmux_open").length;
    say({ type: "exit", ended: { attached: false, reason: "can't find session: $3" } });
    await settle();
    expect(tmux.status).toBe("detached");
    expect(tmux.problem).toBe("can't find session: $3");
    expect(asked("tmux_open").length).toBe(opened);
  });

  it("comes back to its session when a dropped connection is up again", async () => {
    backend.sessions = [
      { id: 3, name: "main", windows: 1, attached: 0 },
      { id: 5, name: "build", windows: 2, attached: 0 },
    ];
    const tmux = await attached();
    tmux.select(5);
    say({ type: "state", state: state(5) });

    tmux.link("lost");
    say({ type: "exit", ended: { attached: true, reason: null } });
    await settle();
    expect(tmux.status).toBe("offline");

    tmux.link("connected");
    await settle();
    expect(asked("tmux_open").at(-1)!.args).toMatchObject({ session: 5 });
  });

  it("forgets everything when the window leaves the remote", async () => {
    const tmux = await attached();
    tmux.link("local");
    expect(tmux.status).toBe("idle");
    expect(tmux.state).toBeNull();
    expect(asked("tmux_close").length).toBeGreaterThan(0);
    // What the attachment it left says late is not heard.
    say({ type: "state", state: state(3) });
    expect(tmux.state).toBeNull();
  });

  it("brings a window to the front at once, and tells tmux", async () => {
    const tmux = await attached();
    const both = state(3);
    both.windows.push({ ...both.windows[0], id: 8, index: 1, active: false });
    say({ type: "state", state: both });

    tmux.show(8);
    expect(tmux.window?.id).toBe(8);
    expect(actions().at(-1)).toEqual({ do: "selectWindow", window: 8, session: 3 });
    tmux.cycle(1);
    expect(tmux.window?.id).toBe(7);
  });

  it("goes to a window of another session by way of its session", async () => {
    const tmux = await attached();
    tmux.show(12, 5);
    expect(actions().at(-1)).toEqual({ do: "selectWindow", window: 12, session: 5 });
    // Not in front until tmux says so: it is not one of this session's.
    expect(tmux.window?.id).toBe(7);
  });
});

describe("what is running in a window", () => {
  const process = (pid: number, ppid: number, command: string, cpu = 0, tty = "pts/3"): Process => ({
    pid,
    ppid,
    tty,
    cpu,
    command,
  });
  const panes = [{ id: 1, pid: 50, tty: "/dev/pts/3" }];

  it("is what was run from each pane's shell, with what that started counted in", () => {
    const found = jobs(panes, [
      process(50, 1, "-bash"),
      process(77, 50, "/usr/bin/make -j8 all", 2),
      process(78, 77, "cc1plus main.cc", 98),
      process(79, 77, "sh -c g++ util.cc", 0),
      process(80, 79, "cc1plus util.cc", 95),
      process(90, 50, "python3 -m http.server", 1),
      // Another terminal's, and so another pane's.
      process(99, 60, "vim notes", 3, "pts/4"),
    ]);
    expect(found).toEqual([
      { pid: 77, pane: 1, command: "make -j8 all", cpu: 195, children: 3 },
      { pid: 90, pane: 1, command: "python3 -m http.server", cpu: 1, children: 0 },
    ]);
  });

  it("is nothing for a shell waiting to be typed at", () => {
    expect(jobs(panes, [process(50, 1, "-bash")])).toEqual([]);
    expect(jobs(panes, [])).toEqual([]);
  });

  it("is the pane's own program where the pane was started on one", () => {
    const found = jobs(panes, [process(50, 1, "/run/current-system/sw/bin/htop -d 5", 4)]);
    expect(found).toEqual([{ pid: 50, pane: 1, command: "htop -d 5", cpu: 4, children: 0 }]);
  });
});

describe("a pane that is busy", () => {
  it("is one running something other than a shell", () => {
    expect(busy(pane(0, { command: "make" }))).toBe(true);
    expect(busy(pane(0, { command: "vim" }))).toBe(true);
    for (const shell of ["bash", "zsh", "fish", "sh", "-bash", "nu"]) {
      expect(busy(pane(0, { command: shell }))).toBe(false);
    }
  });
});
