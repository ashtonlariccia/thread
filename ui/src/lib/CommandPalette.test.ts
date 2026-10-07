// @vitest-environment happy-dom
import { flushSync, mount, unmount } from "svelte";
import { describe, expect, it } from "vitest";

import CommandPalette, { type Command } from "./CommandPalette.svelte";

/** The palette, open on a few commands, with what has been run and whether it has closed. */
function open() {
  const ran: string[] = [];
  let closed = 0;
  const command = (title: string, extra: Partial<Command> = {}): Command => ({
    title,
    run: () => ran.push(title),
    ...extra,
  });
  const target = document.createElement("div");
  document.body.append(target);
  const palette = mount(CommandPalette, {
    target,
    props: {
      open: true,
      commands: [
        command("File: New File", { keys: "Ctrl+N" }),
        command("File: Save", { disabled: true }),
        command("Terminal: New Terminal"),
        command("View: Split Right"),
      ],
      onclose: () => closed++,
    },
  });
  flushSync();

  const input = target.querySelector("input")!;
  const type = (text: string) => {
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  };
  const press = (key: string) => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
    flushSync();
  };
  const rows = () => [...target.querySelectorAll(".row")].map((row) => row.querySelector(".title")!.textContent!.trim());
  const picked = () => target.querySelector(".row.picked .title")?.textContent?.trim();
  return { target, ran, closed: () => closed, type, press, rows, picked, done: () => void unmount(palette) };
}

describe("the command palette", () => {
  it("lists everything until something is typed, with the first picked out", () => {
    const palette = open();
    expect(palette.rows()).toHaveLength(4);
    expect(palette.picked()).toBe("File: New File");
    expect(palette.target.querySelector(".keys")?.textContent).toBe("Ctrl+N");
    palette.done();
  });

  it("narrows to what matches, and runs the one picked on Enter", () => {
    const palette = open();
    palette.type("nt");
    expect(palette.picked()).toBe("Terminal: New Terminal");
    palette.press("Enter");
    expect(palette.ran).toEqual(["Terminal: New Terminal"]);
    expect(palette.closed()).toBe(1);
    palette.done();
  });

  it("moves the pick with the arrows, round the ends", () => {
    const palette = open();
    palette.press("ArrowUp");
    expect(palette.picked()).toBe("View: Split Right");
    palette.press("ArrowDown");
    palette.press("ArrowDown");
    expect(palette.picked()).toBe("File: Save");
    palette.done();
  });

  it("lists what cannot be done just now, and does not do it", () => {
    const palette = open();
    palette.type("save");
    palette.press("Enter");
    expect(palette.ran).toEqual([]);
    expect(palette.closed()).toBe(0);
    palette.done();
  });

  it("closes on Escape having run nothing, and says so when nothing matches", () => {
    const palette = open();
    palette.type("zzz");
    expect(palette.target.querySelector(".none")).not.toBeNull();
    palette.press("Escape");
    expect(palette.closed()).toBe(1);
    expect(palette.ran).toEqual([]);
    palette.done();
  });
});
