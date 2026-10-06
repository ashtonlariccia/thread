import { describe, expect, it } from "vitest";

import { accentVars, isHexColor } from "./accent";

describe("accentVars", () => {
  it("derives the washes from the one colour", () => {
    const vars = accentVars("#89b4fa");
    expect(vars["--accent"]).toBe("#89b4fa");
    expect(vars["--accent-soft"]).toBe("#89b4fa26");
    expect(vars["--hover-strong"]).toBe("#89b4fa33");
  });

  it("sets dark ink on a pale accent and light ink on a deep one", () => {
    expect(accentVars("#cba6f7")["--accent-ink"]).toBe("#1c1c1c");
    expect(accentVars("#f9e2af")["--accent-ink"]).toBe("#1c1c1c");
    expect(accentVars("#1e3a8a")["--accent-ink"]).toBe("#f4f4f4");
    expect(accentVars("#7c3aed")["--accent-ink"]).toBe("#f4f4f4");
  });

  it("changes nothing for something that is not a colour", () => {
    expect(accentVars("mauve")).toEqual({});
    expect(accentVars("#fff")).toEqual({});
  });
});

describe("isHexColor", () => {
  it("is six hex digits after a hash, in either case", () => {
    expect(isHexColor("#CBA6F7")).toBe(true);
    expect(isHexColor("#cba6f7")).toBe(true);
    expect(isHexColor("cba6f7")).toBe(false);
    expect(isHexColor("#cba6f7ff")).toBe(false);
  });
});
