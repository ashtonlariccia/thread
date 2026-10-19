import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: async () => [] }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ message: async () => {} }));

import { Workflows } from "./workflows.svelte";

describe("Workflows", () => {
  it("starts with one, in front", () => {
    const flows = new Workflows();
    expect(flows.list).toHaveLength(1);
    expect(flows.current.id).toBe(flows.list[0].id);
    expect(flows.label(flows.current)).toBe("Workflow 1");
  });

  it("gives each its own panes, with ids no other has", () => {
    const flows = new Workflows();
    const second = flows.add();
    expect(second.layout).not.toBe(flows.list[0].layout);
    expect(second.tree).not.toBe(flows.list[0].tree);
    const ids = flows.list.map((flow) => flow.layout.leaves[0].id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("goes by its own name, then its first folder", () => {
    const flows = new Workflows();
    const flow = flows.current;
    flow.tree.roots.push({ name: "rustlings", path: "C:\\src\\rustlings", dir: true });
    expect(flows.label(flow)).toBe("rustlings");
    flows.rename(flow.id, "  exercises ");
    expect(flows.label(flow)).toBe("exercises");
    flows.rename(flow.id, " ");
    expect(flows.label(flow)).toBe("rustlings");
  });

  it("steps round, and closing one brings the one before it to the front", () => {
    const flows = new Workflows();
    const [a, b, c] = [flows.current, flows.add(), flows.add()];
    flows.step(-1);
    expect(flows.current.id).toBe(c.id);
    flows.step(1);
    expect(flows.current.id).toBe(a.id);

    flows.select(b.id);
    flows.remove(b.id);
    expect(flows.current.id).toBe(a.id);
    expect(flows.list.map((flow) => flow.id)).toEqual([a.id, c.id]);
  });

  it("never closes the last", () => {
    const flows = new Workflows();
    flows.remove(flows.current.id);
    expect(flows.list).toHaveLength(1);
  });
});
