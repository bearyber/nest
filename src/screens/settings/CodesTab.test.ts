import { describe, expect, it } from "vitest";
import { CODE_STYLES, codeParts, previewCode } from "./CodesTab";

describe("previewCode", () => {
  it("fills client, letter and number", () => {
    expect(previewCode("{clientCode}-{marker}{seq:02}", "VX", "w", 4)).toBe("VX-W04");
    expect(previewCode("{clientCode}-{marker}{seq:03}", "VX", "M", 7)).toBe("VX-M007");
    expect(previewCode("{clientCode}{seq}", "OWN", "W", 12)).toBe("OWN12");
    expect(previewCode("{clientCode}-{marker}{seq:02}", "VX", "W", 100)).toBe("VX-W100");
  });
});

describe("codeParts", () => {
  it("labels each part so the example can explain it", () => {
    expect(codeParts("{clientCode}-{marker}{seq:02}", "VX", "W", 4)).toEqual([
      { kind: "client", text: "VX" },
      { kind: "text", text: "-" },
      { kind: "marker", text: "W" },
      { kind: "seq", text: "04" },
    ]);
  });

  it("keeps a stray brace as text instead of dropping it", () => {
    expect(previewCode("{clientCode}{x", "VX", "W", 1)).toBe("VX{x");
  });
});

describe("CODE_STYLES", () => {
  it("every ready-made style keeps the computer's letter", () => {
    for (const s of CODE_STYLES) expect(s.pattern).toContain("{marker}");
  });
});
