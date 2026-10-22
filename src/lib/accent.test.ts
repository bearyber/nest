// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { NEST_BLUE, accentTokens, applyAccent, contrast, followSystemAccent, mix, parseHex } from "./accent";

let accentAnswer: (hex: string | null) => void = () => {};
let accentEvent: (payload: string | null) => void = () => {};
vi.mock("./commands", () => ({
  systemAccent: () => new Promise<string | null>((resolve) => (accentAnswer = resolve)),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, cb: (e: { payload: string | null }) => void) => {
    accentEvent = (payload) => cb({ payload });
    return Promise.resolve(() => {});
  },
}));

const rgb = (hex: string) => parseHex(hex)!;
const rgba = (css: string) => css.match(/\d+(\.\d+)?/g)!.map(Number);

// macOS accent choices (light variants as AppKit reports them) plus a few Windows-style ones.
const ACCENTS = {
  blue: "#007aff",
  purple: "#953d96",
  pink: "#f74f9e",
  red: "#e0383e",
  orange: "#f7821b",
  yellow: "#ffc600",
  green: "#62ba46",
  graphite: "#989898",
  windowsBlue: "#0078d4",
  white: "#ffffff",
  black: "#000000",
};

describe("accent shades", () => {
  it("Nest blue gives the blue the stylesheet falls back to", () => {
    const t = accentTokens(NEST_BLUE);
    expect(t.accentLight).toBe("#0a84ff");
    // Within one step of the fallback values in tokens.css (rounding).
    const near = (a: string, b: string) => rgb(a).every((v, i) => Math.abs(v - rgb(b)[i]) <= 1);
    expect(near(t.textLight, "#075cb2")).toBe(true);
    expect(near(t.textDark, "#6cb5ff")).toBe(true);
    expect(near(t.fill, "#0863bf")).toBe(true);
    expect(near(t.fillHover, "#075aad")).toBe(true);
  });

  it("every shade is readable for every accent, light and dark", () => {
    for (const [name, hex] of Object.entries(ACCENTS)) {
      const t = accentTokens(hex);
      expect(contrast(rgb(t.textLight), rgb("#f3f3f3")), `${name} text, light`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(rgb(t.textLight), rgb("#ffffff")), `${name} text on white`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(rgb(t.textDark), rgb("#34363a")), `${name} text, dark`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(rgb(t.textDark), rgb("#202020")), `${name} text on dark window`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(rgb(t.fill), rgb("#ffffff")), `${name} fill`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(rgb(t.fillHover), rgb("#ffffff")), `${name} fill hover`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("keeps the OS colour's own shades where they read (purple, red)", () => {
    for (const hex of [ACCENTS.purple, ACCENTS.red]) {
      const t = accentTokens(hex);
      expect(t.fill).not.toBe(accentTokens(NEST_BLUE).fill);
      expect(t.textLight).toBe(
        "#" +
          mix(rgb(hex), [0, 0, 0], 30)
            .map((v) => v.toString(16).padStart(2, "0"))
            .join(""),
      );
    }
  });

  it("falls back to Nest blue's fill for yellow (white text on a yellow fill can't be read)", () => {
    const t = accentTokens(ACCENTS.yellow);
    const blue = accentTokens(NEST_BLUE);
    expect(t.fill).toBe(blue.fill);
    expect(t.fillHover).toBe(blue.fillHover);
    // Rings and dots need 3:1 too: yellow can't be seen on a light window (blue there), but
    // reads fine on a dark one.
    expect(t.accentLight).toBe(NEST_BLUE);
    expect(t.accentDark).toBe(ACCENTS.yellow);
  });

  it("rings and dots follow the OS where they can be seen", () => {
    expect(accentTokens(ACCENTS.purple).accentLight).toBe(ACCENTS.purple);
    // Light grey rings vanish on a light window (blue there); they read on a dark one.
    expect(accentTokens(ACCENTS.graphite).accentLight).toBe(NEST_BLUE);
    expect(accentTokens(ACCENTS.graphite).accentDark).toBe(ACCENTS.graphite);
  });

  it("graphite and pink: shades are either theirs or blue's, never unreadable", () => {
    for (const hex of [ACCENTS.graphite, ACCENTS.pink]) {
      const t = accentTokens(hex);
      const blue = accentTokens(NEST_BLUE);
      expect([mix(rgb(hex), [0, 0, 0], 25).join(), rgb(blue.fill).join()]).toContain(rgb(t.fill).join());
    }
    // Graphite's light-mode text shade is fine as its own; the dark one is lighter than graphite.
    expect(accentTokens(ACCENTS.graphite).textLight).not.toBe(accentTokens(NEST_BLUE).textLight);
  });

  it("white and black accents don't break anything", () => {
    for (const hex of [ACCENTS.white, ACCENTS.black]) expect(accentTokens(hex).fill).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("soft tints use the accent at 12% (light) and 20% (dark)", () => {
    const t = accentTokens(ACCENTS.purple);
    expect(rgba(t.softLight)).toEqual([149, 61, 150, 0.12]);
    expect(rgba(t.softDark)).toEqual([149, 61, 150, 0.2]);
  });

  it("when the fill falls back to blue, the soft tints do too (one hue in a selection)", () => {
    const t = accentTokens(ACCENTS.white);
    expect(rgb(t.fill).join()).toBe(rgb(accentTokens(NEST_BLUE).fill).join());
    expect(rgba(t.softLight)).toEqual([10, 132, 255, 0.12]);
    expect(rgba(t.softDark)).toEqual([10, 132, 255, 0.2]);
  });

  it("a bad colour reads as Nest blue", () => {
    expect(accentTokens("nonsense").accentLight).toBe("#0a84ff");
  });
});

describe("applying the accent", () => {
  it("sets the variables on the page, and removes them for null", () => {
    const root = document.createElement("div");
    applyAccent("#953d96", root);
    expect(root.style.getPropertyValue("--accent-light")).toBe("#953d96");
    expect(root.style.getPropertyValue("--accent-fill-value")).toMatch(/^#[0-9a-f]{6}$/);
    applyAccent(null, root);
    expect(root.style.getPropertyValue("--accent")).toBe("");
    expect(root.style.getPropertyValue("--accent-soft-dark")).toBe("");
    applyAccent("garbage", root);
    expect(root.style.getPropertyValue("--accent")).toBe("");
  });
});

describe("following the system accent", () => {
  it("a change that arrives before the first answer is not overwritten by it", async () => {
    document.documentElement.removeAttribute("style");
    const stop = followSystemAccent();
    await Promise.resolve();
    accentEvent("#953d96");
    accentAnswer("#0078d4"); // the late, older answer
    await new Promise((r) => setTimeout(r, 0));
    expect(document.documentElement.style.getPropertyValue("--accent-light")).toBe("#953d96");
    stop();
  });

  it("the first answer is used when nothing newer arrived", async () => {
    document.documentElement.removeAttribute("style");
    const stop = followSystemAccent();
    await Promise.resolve();
    accentAnswer("#953d96");
    await new Promise((r) => setTimeout(r, 0));
    expect(document.documentElement.style.getPropertyValue("--accent-light")).toBe("#953d96");
    stop();
  });
});
