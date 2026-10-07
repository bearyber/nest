import { describe, expect, it } from "vitest";
import {
  followMoves,
  fromBlocks,
  fromNodes,
  locate,
  moveNode,
  newKey,
  removeQuestion,
  toBlocks,
  toNodes,
  usage,
} from "./templateDraft";
import type { Template, TreeEntry } from "./types";

const bundled = Object.values(
  import.meta.glob<Template>("../../src-tauri/templates/*/*.nest.json", { eager: true, import: "default" }),
);

// Bernard's Grading: MV tree (private/extras/bernard-templates): nested paths, "only if", "one per".
const MV_TREE: TreeEntry[] = [
  "00_REFERENCE/BRIEF",
  "00_REFERENCE/MOODBOARD",
  "01_FOOTAGES",
  "03_PROJECT",
  "04_WIP",
  { name: "05_FINAL/{item}", each: "ratios" },
  "06_DI/CDL",
  "06_DI/GRADE_STILLS",
  "06_DI/LUTS",
  { name: "06_DI/LOOKDEV", when: "lookdev" },
  "09_STILLS",
];

describe("folder tree", () => {
  it("round-trips every template's folders unchanged (no extra parent entries)", () => {
    for (const tree of [...bundled.map((t) => t.tree ?? []), MV_TREE]) {
      const back = fromNodes(toNodes(tree));
      expect(back.problems).toEqual([]);
      expect(back.tree).toEqual(tree);
    }
  });

  it("keeps a folder with both rules and a 'not ticked' rule", () => {
    const tree: TreeEntry[] = [
      { name: "A/{item}", when: "gfx", each: "formats" },
      { name: "NO_GFX", when: "!gfx" },
    ];
    expect(fromNodes(toNodes(tree)).tree).toEqual(tree);
  });

  it("a folder moved inside an 'only if' folder inherits the rule", () => {
    let nodes = toNodes(["01_BRIEF", { name: "04_GRAPHICS", when: "graphics" }]);
    const brief = nodes[0].id;
    const gfx = nodes[1].id;
    nodes = moveNode(nodes, brief, gfx, "inside");
    expect(fromNodes(nodes).tree).toEqual([
      { name: "04_GRAPHICS", when: "graphics" },
      { name: "04_GRAPHICS/01_BRIEF", when: "graphics" },
    ]);
  });

  it("refuses two different 'only if' rules on one path, in plain words", () => {
    const nodes = toNodes([{ name: "A", when: "x" }]);
    nodes[0].kids.push({ id: "k", name: "B", when: "y", explicit: true, kids: [] });
    const r = fromNodes(nodes, (k) => (k === "x" ? "Has graphics" : k));
    expect(r.problems[0]).toContain('"A/B"');
    expect(r.problems[0]).toContain("Has graphics");
  });

  it("moves before, after and inside, but never into itself", () => {
    const nodes = toNodes(["A", "B", "C/D"]);
    const [a, b, c] = nodes.map((n) => n.id);
    const d = nodes[2].kids[0].id;
    expect(fromNodes(moveNode(nodes, a, c, "after")).tree).toEqual(["B", "C/D", "A"]);
    expect(fromNodes(moveNode(nodes, c, a, "before")).tree).toEqual(["C/D", "A", "B"]);
    // D stays listed as its own folder (harmless: Nest makes each folder once).
    expect(fromNodes(moveNode(nodes, b, d, "inside")).tree).toEqual(["A", "C/D", "C/D/B"]);
    expect(moveNode(nodes, c, d, "inside")).toBe(nodes); // C into its own child: refused
  });

  it("starter files follow a renamed or moved folder", () => {
    const before = toNodes(["DOCS/BRIEF", "OTHER"]);
    const after = structuredClone(before);
    locate(after, after[0].id)!.node.name = "00_DOCS";
    expect(followMoves([{ from: "starter/a.md", to: "DOCS/BRIEF/a.md" }, { from: "n", to: "NOTES.md" }], before, after)).toEqual([
      { from: "starter/a.md", to: "00_DOCS/BRIEF/a.md" },
      { from: "n", to: "NOTES.md" },
    ]);
  });
});

describe("folder name blocks", () => {
  it("reads and writes simple patterns", () => {
    expect(toBlocks("{start|yymmdd}_{name|caps}")).toEqual([
      { key: "start", transform: "yymmdd" },
      { key: "name", transform: "caps" },
    ]);
    expect(fromBlocks([{ key: "start", transform: "yymmdd" }, { key: "name" }])).toBe("{start|yymmdd}_{name}");
    expect(toBlocks("{artist} {song}", " ")).toEqual([{ key: "artist" }, { key: "song" }]);
  });

  it("leaves patterns with literal text to Advanced (null), so nothing is lost", () => {
    expect(toBlocks("{start|yymmdd}_{artist|caps}_PV")).toBeNull();
    expect(toBlocks("{yymm}-{jobCode}")).toBeNull();
  });
});

describe("questions", () => {
  it("makes keys from labels: unique, never a built-in", () => {
    expect(newKey("Has graphics?", [])).toBe("has_graphics");
    expect(newKey("Date", [])).toBe("date_2"); // built-in
    expect(newKey("Formats", ["formats"])).toBe("formats_2");
    expect(newKey("뮤직비디오", [])).toBe("question");
    expect(newKey("Café", [])).toBe("cafe");
  });

  it("knows where a question is used and removes it everywhere", () => {
    const video = bundled.find((t) => t.id === "video")!;
    expect(usage(video, "name").inFolderName).toBe(true);
    expect(usage(video, "formats").folders).toBe(1);
    expect(usage(video, "graphics").folders).toBe(1);

    const noGraphics = removeQuestion(video, "graphics");
    expect(noGraphics.fields.map((f) => f.key)).not.toContain("graphics");
    expect(noGraphics.tree).toContain("06_GRAPHICS"); // the folder stays, made always

    const noFormats = removeQuestion(video, "formats");
    expect(JSON.stringify(noFormats.tree)).not.toContain("{item}"); // "one per" folders go

    const noName = removeQuestion(video, "name");
    expect(noName.folderName).toBe("{start|yymmdd}");
  });
});

describe("retype", () => {
  it("keeps the limit for long text and starting picks between pick one / pick several", async () => {
    const { retype } = await import("./templateDraft");
    expect(retype({ key: "n", label: "N", type: "text", max: 40 }, "longtext").max).toBe(40);
    const multi = retype({ key: "f", label: "F", type: "select", options: ["a", "b"], default: "b" }, "multi");
    expect(multi.default).toEqual(["b"]);
    expect(retype(multi, "select").default).toBe("b");
    expect(retype({ key: "d", label: "D", type: "date", default: "today" }, "multi").default).toBeUndefined();
  });

  it("drops Required for pick several and yes/no (their switch is off)", async () => {
    const { retype } = await import("./templateDraft");
    const req = { key: "n", label: "N", type: "text" as const, required: true };
    expect(retype(req, "multi").required).toBeUndefined();
    expect(retype(req, "bool").required).toBeUndefined();
    expect(retype(req, "select").required).toBe(true);
  });
});

describe("removeQuestion with a custom pattern", () => {
  it("removes the answer from a name the blocks can't show", () => {
    const t: Template = {
      schema: 1,
      id: "x",
      version: 1,
      name: "X",
      description: "",
      fields: [
        { key: "client", label: "Billed to", type: "client" },
        { key: "artist", label: "Artist", type: "text" },
        { key: "song", label: "Song", type: "text" },
      ],
      folderName: "{start|yymmdd}_{artist|caps}_{song|caps}_PV",
      tree: [],
    };
    expect(removeQuestion(t, "song").folderName).toBe("{start|yymmdd}_{artist|caps}_PV");
  });
});

describe("followMoves never mixes two folders' files", () => {
  it("leaves files alone when the new path belonged to another folder", () => {
    const before = toNodes(["AUDIO", "AUDIO_FINAL"]);
    const after = structuredClone(before);
    locate(after, after[1].id)!.node.name = "AUDIO"; // renamed onto its sibling's name
    const files = [
      { from: "a", to: "AUDIO/a.txt" },
      { from: "b", to: "AUDIO_FINAL/b.txt" },
    ];
    expect(followMoves(files, before, after)).toEqual(files);
  });
});
