import { describe, expect, it } from "vitest";
import {
  createdMessage,
  libraryKey,
  outcomeWords,
  plannedLine,
  projectLine,
  sameLibrary,
  statusLine,
  toastForNotice,
} from "./resolve";

describe("Resolve words", () => {
  it("preview line: folders inside folders, or the top level", () => {
    const r = { drp: "files/G.drp", name: "Grade", folder: "Acme/2026", library: null, libraryLabel: "x" };
    expect(plannedLine(r)).toBe("Resolve project: Grade in Acme / 2026");
    expect(plannedLine({ ...r, folder: "" })).toBe("Resolve project: Grade in the top level");
    expect(plannedLine({ ...r, name: "한국 프로젝트", folder: "客户" })).toBe("Resolve project: 한국 프로젝트 in 客户");
  });

  it("details line for each status", () => {
    const r = { name: "Grade", folder: "", library: null, status: "added", at: "2026-10-17T10:00:00+02:00" };
    const added = projectLine(r);
    expect(added).toMatch(/^Resolve project: Grade · added /);
    expect(added).toMatch(/17/);
    expect(added).toMatch(/Oct/);
    expect(projectLine({ ...r, at: "" })).toBe("Resolve project: Grade · added");
    expect(projectLine({ ...r, status: "waiting" })).toBe("Resolve project: Grade · waiting for Resolve");
    expect(projectLine({ ...r, status: "exists" })).toBe("Resolve project: Grade · already in Resolve");
    expect(projectLine({ ...r, status: "skipped" })).toMatch(/not added/);
    expect(projectLine({ ...r, status: "failed" })).toMatch(/stopped trying/);
    expect(projectLine({ ...r, status: "somethingNew" })).toBe("Resolve project: Grade");
  });

  it("last result words, unknown ones as they are", () => {
    expect(outcomeWords(null)).toBe("Not tried yet");
    expect(outcomeWords("notRunning")).toBe("Resolve wasn't open");
    expect(outcomeWords("busy")).toMatch(/another library/);
    expect(outcomeWords("later")).toBe("later");
  });

  it("status line: ✓ when ready; the scripting hint allows for Resolve just opening", () => {
    expect(statusLine("ready")).toEqual({ text: "✓ Resolve is ready", ok: true });
    expect(statusLine("scriptingOff").text).toMatch(/External scripting using.*Local/);
    expect(statusLine("scriptingOff").text).toMatch(/If Resolve has just opened, wait a few seconds\.$/);
    expect(statusLine("notRunning").text).toMatch(/^Open Resolve to check/);
    expect(statusLine("notInstalled").text).toMatch(/isn't installed/);
  });

  it("libraries compare by type and name; none = the open one", () => {
    const a = { dbType: "Disk", dbName: "Work" };
    expect(libraryKey(null)).toBe("");
    expect(sameLibrary(a, { ...a })).toBe(true);
    expect(sameLibrary(a, { dbType: "PostgreSQL", dbName: "Work" })).toBe(false);
    expect(sameLibrary(null, null)).toBe(true);
  });

  it("success line after Create", () => {
    expect(createdMessage("VX-L01", false, null)).toBe("Created VX-L01");
    expect(createdMessage("VX-L01", false, "waiting")).toBe("Created VX-L01 · Ready when Resolve opens");
    expect(createdMessage("VX-L02", true, "added")).toBe(
      "Created VX-L02 (the previewed code was taken) · Added to Resolve",
    );
  });

  it("the added notice upgrades the success line only while it shows", () => {
    const toast = { message: "Created VX-L01 · Ready when Resolve opens" };
    const added = { message: "Created VX-L01 · Added to Resolve" };
    const pending = { toast, name: "KIRA", added };
    expect(toastForNotice(toast, pending, "Added to Resolve: KIRA")).toBe(added);
    // Another project's notice, another notice, or the line already gone: the notice itself.
    expect(toastForNotice(toast, pending, "Added to Resolve: Other")).toEqual({ message: "Added to Resolve: Other" });
    expect(toastForNotice(null, pending, "Added to Resolve: KIRA")).toEqual({ message: "Added to Resolve: KIRA" });
    expect(toastForNotice(toast, null, "Templates synced")).toEqual({ message: "Templates synced" });
  });
});
