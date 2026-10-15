import { describe, expect, it } from "vitest";
import { folderNote } from "./Templates";

describe("folderNote", () => {
  it("counts what was left out in plain words, without system files", () => {
    const note = folderNote(
      [
        { path: ".DS_Store", reason: "clutter" },
        { path: "A/clip.mov", reason: "tooBig" },
        { path: "B/clip.mov", reason: "tooBig" },
        { path: "P/Adobe Premiere Pro Auto-Save", reason: "cache" },
      ],
      false,
    );
    expect(note).toBe("Left out: 2 files over 100 MB, 1 cache, auto-save or app library folder. Your folder wasn't changed.");
  });

  it("says when everything came along, and when answers became blanks", () => {
    expect(folderNote([{ path: "Thumbs.db", reason: "clutter" }], true)).toBe(
      "This was a Nest project: its answers became questions New Project asks. Everything came along. Your folder wasn't changed.",
    );
  });
});
