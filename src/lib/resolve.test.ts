import { describe, expect, it } from "vitest";
import { createdMessage, libraryKey, plannedLine, projectLine, sameLibrary, toastForNotice } from "./resolve";

describe("Resolve words", () => {
  it("preview line: says it adds to Resolve, and where (library only when the template picks one)", () => {
    const r = { drp: "files/G.drp", name: "Grade", folder: "Acme/2026", library: null, libraryLabel: "x" };
    expect(plannedLine(r)).toBe("Adds “Grade” to DaVinci Resolve, in Acme › 2026");
    expect(plannedLine({ ...r, library: { dbType: "Disk", dbName: "Client Work" } })).toBe(
      "Adds “Grade” to DaVinci Resolve, in Client Work › Acme › 2026",
    );
    expect(plannedLine({ ...r, folder: "", library: null })).toBe("Adds “Grade” to DaVinci Resolve");
    expect(plannedLine({ ...r, name: "한국 프로젝트", folder: "客户" })).toBe("Adds “한국 프로젝트” to DaVinci Resolve, in 客户");
  });

  it("details line for each status", () => {
    const r = {
      name: "Grade",
      folder: "",
      library: null,
      status: "added",
      at: "2026-10-17T10:00:00+02:00",
      note: null,
      waiting: false,
      problem: null,
    };
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
