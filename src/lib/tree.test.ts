import { describe, expect, it } from "vitest";
import { buildTree, defaultsFor } from "./tree";

describe("buildTree", () => {
  it("nests folders and puts files inside them", () => {
    const tree = buildTree({
      folders: ["01_REF", "07_DELIVERY", "07_DELIVERY/16x9", "04_LUTS/Film"],
      files: [
        { from: "a", to: "NOTES.md", fill: true },
        { from: "b", to: "04_LUTS/Film/Kodak.cube", fill: false },
      ],
    });
    expect(tree.map((n) => n.name)).toEqual(["01_REF", "07_DELIVERY", "04_LUTS", "NOTES.md"]);
    expect(tree[1].children.map((n) => n.name)).toEqual(["16x9"]);
    const film = tree[2].children[0];
    expect(film.children[0]).toEqual({ name: "Kodak.cube", isFile: true, children: [] });
    expect(tree[3].isFile).toBe(true);
  });

  it("shows the client kit's first files inside its folder and counts the rest", () => {
    const tree = buildTree({
      folders: ["02_BRAND", "02_BRAND/APL"],
      files: [],
      kit: {
        state: "ready",
        client: "Apple",
        code: "APL",
        folder: "02_BRAND/APL",
        count: 40,
        bytes: 1,
        sample: ["02_BRAND/APL/Logos/logo.png", "02_BRAND/APL/Guide.pdf"],
      },
    });
    const apl = tree[0].children[0];
    expect(apl.children.map((n) => n.name)).toEqual(["Logos", "Guide.pdf", "…and 38 more files"]);
    expect(apl.children[0].children[0].name).toBe("logo.png");
  });

  it("leaves a kit out unless it's ready to copy", () => {
    const kit = { state: "missing" as const, client: "Apple", code: "APL", folder: "B", count: 0, bytes: 0, sample: ["B/x"] };
    expect(buildTree({ folders: ["B"], files: [], kit })[0].children).toEqual([]);
  });
});

describe("defaultsFor", () => {
  it("takes each field's default", () => {
    expect(
      defaultsFor([
        { key: "artist", label: "Artist", type: "text" },
        { key: "ratios", label: "Ratios", type: "multi", options: ["16x9"], default: ["16x9"] },
        { key: "lookdev", label: "Look dev", type: "bool", default: false },
      ]),
    ).toEqual({ ratios: ["16x9"], lookdev: false });
  });

  it("turns a date default of 'today' into the local date", () => {
    expect(
      defaultsFor(
        [{ key: "start", label: "Start date", type: "date", default: "today" }],
        new Date(2025, 5, 8),
      ),
    ).toEqual({ start: "2025-06-08" });
  });
});
