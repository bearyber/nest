// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { areaOf, trackFocusArea } from "./focusArea";

describe("focus area (which selection is blue)", () => {
  it("names the area an element is in, else other", () => {
    document.body.innerHTML = `<nav data-area="sidebar"><button id="a">x</button></nav><div data-area="list"><span id="b"></span></div><aside id="c"></aside>`;
    expect(areaOf(document.getElementById("a"))).toBe("sidebar");
    expect(areaOf(document.getElementById("b"))).toBe("list");
    expect(areaOf(document.getElementById("c"))).toBe("other");
    expect(areaOf(null)).toBe("other");
  });

  it("follows clicks and focus, starting on the list", () => {
    document.body.innerHTML = `<nav data-area="sidebar"><button id="a">x</button></nav><div data-area="list"><span id="b"></span></div>`;
    const root = document.createElement("html");
    const stop = trackFocusArea(root, document);
    expect(root.dataset.area).toBe("list");
    document.getElementById("a")!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(root.dataset.area).toBe("sidebar");
    document.getElementById("b")!.dispatchEvent(new Event("focusin", { bubbles: true }));
    expect(root.dataset.area).toBe("list");
    stop();
    document.getElementById("a")!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    expect(root.dataset.area).toBe("list");
  });
});
