import { describe, expect, it } from "vitest";
import {
  addFolder,
  blanksIn,
  filesIn,
  formatName,
  indent,
  insertBlank,
  makeBlank,
  outdent,
  parentOf,
  parseName,
  record,
  redo,
  removeFolder,
  startHistory,
  undo,
  UNDO_STEPS,
  withoutFilesIn,
} from "./editorModel";
import { fromNodes, toNodes, type FolderNode } from "./templateDraft";

const names = (nodes: FolderNode[]) => fromNodes(nodes).tree.map((e) => (typeof e === "string" ? e : e.name));
const idOf = (nodes: FolderNode[], path: string): string => {
  let list = nodes;
  let found: FolderNode | undefined;
  for (const seg of path.split("/")) {
    found = list.find((n) => n.name === seg);
    if (!found) throw new Error(`no ${path}`);
    list = found.kids;
  }
  return found!.id;
};

describe("names", () => {
  it("round-trip text and blanks", () => {
    for (const s of ["{start|yymmdd}_{name|caps}", "EXPORTS", "{jobCode}", "A_{x}_B", "", "좋은_{name}"]) {
      expect(formatName(parseName(s))).toBe(s);
    }
    expect(parseName("{start|yymmdd}_{name}")).toEqual([
      { key: "start", transform: "yymmdd" },
      { text: "_" },
      { key: "name" },
    ]);
  });

  it("keep a half-typed blank as text", () => {
    expect(parseName("C_CAM_{")).toEqual([{ text: "C_CAM_{" }]);
    expect(formatName(parseName("{a}_{b"))).toBe("{a}_{b");
  });

  it("insert a blank where the cursor is", () => {
    expect(insertBlank("C_CAM_", 6, "shoot")).toBe("C_CAM_{shoot}");
    expect(insertBlank("_v1.prproj", 0, "name", "caps")).toBe("{name|caps}_v1.prproj");
  });

  it("make a blank from a selection, never leaving half a blank", () => {
    expect(makeBlank("NIKE_EXPORTS", 0, 4, "clientCode")).toBe("{clientCode}_EXPORTS");
    expect(makeBlank("NIKE_EXPORTS", 4, 0, "clientCode")).toBe("{clientCode}_EXPORTS"); // backwards selection
    expect(makeBlank("{start}_NIKE", 3, 12, "client")).toBe("{client}"); // cuts into {start}: widened
  });

  it("list the blanks a name uses once each", () => {
    expect(blanksIn("{a}_{b|caps}_{a}")).toEqual(["a", "b"]);
  });
});

describe("outline moves", () => {
  const tree = () => toNodes(["01_BRIEF", "02_FOOTAGE", "02_FOOTAGE/A_CAM", "02_FOOTAGE/B_CAM", { name: "03_EXPORTS/{item}", each: "formats" }]);

  it("add a folder inside any folder, or after one", () => {
    const t = tree();
    const inside = addFolder(t, idOf(t, "02_FOOTAGE/B_CAM"), "PROXIES");
    expect(names(inside.nodes)).toContain("02_FOOTAGE/B_CAM/PROXIES");
    const after = addFolder(t, idOf(t, "02_FOOTAGE"), "A2_CAM", idOf(t, "02_FOOTAGE/A_CAM"));
    expect(names(after.nodes)).toEqual(["01_BRIEF", "02_FOOTAGE", "02_FOOTAGE/A_CAM", "02_FOOTAGE/A2_CAM", "02_FOOTAGE/B_CAM", "03_EXPORTS/{item}"]);
    const top = addFolder(t, null, "04_GRAPHICS");
    const all = names(top.nodes);
    expect(all[all.length - 1]).toBe("04_GRAPHICS");
    expect(names(t)).toEqual(names(tree())); // the original is untouched
  });

  it("never add fixed folders inside a 'one per' folder", () => {
    const t = tree();
    const each = idOf(t, "03_EXPORTS/{item}");
    expect(addFolder(t, each, "X").id).toBeNull();
  });

  it("indent into the folder above, outdent back out", () => {
    const t = tree();
    const brief = idOf(t, "01_BRIEF");
    expect(indent(t, brief)).toBeNull(); // nothing above
    const footage = idOf(t, "02_FOOTAGE");
    const inBrief = indent(t, footage)!;
    expect(names(inBrief)).toContain("01_BRIEF/02_FOOTAGE/A_CAM");
    expect(parentOf(inBrief, footage)).toBe(brief);
    const out = outdent(inBrief, footage)!;
    expect(names(out)).toEqual(names(t));
    expect(outdent(t, brief)).toBeNull(); // already at the top
  });

  it("won't indent into a 'one per' folder", () => {
    const t = toNodes([{ name: "{item}", each: "formats" }, "B"]);
    expect(indent(t, idOf(t, "B"))).toBeNull();
  });

  it("delete a folder with everything in it, and its files", () => {
    const t = tree();
    expect(names(removeFolder(t, idOf(t, "02_FOOTAGE")))).toEqual(["01_BRIEF", "03_EXPORTS/{item}"]);
    const files = [
      { from: "a", to: "02_FOOTAGE/A_CAM/x.txt", fill: false },
      { from: "b", to: "02_FOOTAGE/notes.md", fill: true },
      { from: "c", to: "NOTES.md", fill: true },
    ];
    expect(withoutFilesIn(files, "02_FOOTAGE").map((f) => f.from)).toEqual(["c"]);
    expect(filesIn(files, "02_FOOTAGE").map((f) => f.from)).toEqual(["b"]);
    expect(filesIn(files, "").map((f) => f.from)).toEqual(["c"]);
  });
});

describe("undo history", () => {
  it("undo and redo steps, and a new step clears redo", () => {
    let h = startHistory("a");
    h = record(h, "b");
    h = record(h, "c");
    h = undo(h);
    expect(h.present).toBe("b");
    h = redo(h);
    expect(h.present).toBe("c");
    h = undo(undo(h));
    expect(h.present).toBe("a");
    expect(undo(h)).toBe(h); // nothing left to undo
    h = record(h, "z");
    expect(redo(h)).toBe(h); // redo cleared
  });

  it(`keeps the last ${UNDO_STEPS} steps`, () => {
    let h = startHistory(0);
    for (let i = 1; i <= UNDO_STEPS + 20; i++) h = record(h, i);
    expect(h.past.length).toBe(UNDO_STEPS);
    expect(h.past[0]).toBe(20);
  });
});
