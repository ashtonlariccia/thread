import { describe, expect, it } from "vitest";

import { baseName, samePath } from "./paths";

describe("baseName", () => {
  it("takes the last component with either slash", () => {
    expect(baseName("C:\\src\\thread\\main.rs")).toBe("main.rs");
    expect(baseName("C:/src/thread/main.rs")).toBe("main.rs");
    expect(baseName("C:\\src/mixed\\main.rs")).toBe("main.rs");
  });

  it("returns a bare name unchanged", () => {
    expect(baseName("main.rs")).toBe("main.rs");
  });
});

describe("samePath", () => {
  it("ignores case and slash direction", () => {
    expect(samePath("C:\\Src\\Main.rs", "c:/src/main.rs")).toBe(true);
  });

  it("tells different files apart", () => {
    expect(samePath("C:\\src\\main.rs", "C:\\src\\lib.rs")).toBe(false);
  });
});
