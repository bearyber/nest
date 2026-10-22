import { describe, expect, it } from "vitest";
import { CODE_STYLES, codeParts, computersWithLetter, previewCode, separatorWarning, withSeparator, yourComputers } from "./CodesTab";

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

describe("separatorWarning", () => {
  it("warns about two letters run together with the client code, either way round", () => {
    expect(separatorWarning("{clientCode}{marker}{seq:02}", "WL")).toContain("ACMEWL04");
    expect(separatorWarning("{marker}{clientCode}{seq:02}", "WL")).toContain("WLACME04");
  });

  it("is quiet for one letter, a dash, or a style without the letter", () => {
    expect(separatorWarning("{clientCode}{marker}{seq:02}", "W")).toBeNull();
    expect(separatorWarning("{clientCode}-{marker}{seq:02}", "WL")).toBeNull();
    expect(separatorWarning("{clientCode}{seq:02}", "WL")).toBeNull();
  });

  it("the fix puts a dash between them and nothing else changes", () => {
    expect(withSeparator("{clientCode}{marker}{seq:02}")).toBe("{clientCode}-{marker}{seq:02}");
    expect(withSeparator("{marker}{clientCode}{seq:03}")).toBe("{marker}-{clientCode}{seq:03}");
    expect(withSeparator("{clientCode}-{marker}{seq:02}")).toBe("{clientCode}-{marker}{seq:02}");
  });
});

describe("letters of other computers", () => {
  const pc = { deviceId: "1", name: "PC", os: "windows", letter: "W", guessed: false };
  it("finds the computers with a letter, whatever the case, and W is not WL", () => {
    expect(computersWithLetter([pc], "w")).toEqual([pc]);
    expect(computersWithLetter([pc], "WL")).toEqual([]);
    expect(computersWithLetter([pc], "")).toEqual([]);
  });

  it("names them as the owner would say it", () => {
    expect(yourComputers(["PC"])).toBe("your PC");
    expect(yourComputers(["PC", "서울 맥"])).toBe("your PC and your 서울 맥");
  });
});
