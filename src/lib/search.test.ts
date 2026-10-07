import { describe, expect, it } from "vitest";
import { filterProjects, fold, matches, sortProjects } from "./search";
import type { ProjectRow } from "./types";

function row(partial: Partial<ProjectRow>): ProjectRow {
  return {
    key: partial.key ?? "k",
    id: "k",
    jobCode: "",
    title: "",
    client: { name: "", code: "" },
    space: "Work",
    status: "active",
    templateId: "project",
    templateName: "Project",
    createdAt: "2026-09-28T10:00:00+08:00",
    fields: {},
    error: null,
    paths: ["D:/Jobs/x"],
    offline: false,
    duplicateCode: false,
    archived: false,
    ...partial,
  };
}

const rows = [
  row({
    key: "a",
    jobCode: "VX-L04",
    title: "KIRA Summer Nights",
    client: { name: "Vertex Studio", code: "VX" },
    fields: { artist: "KIRA", song: "Summer Nights", ratios: ["16x9", "9x16"] },
    createdAt: "2026-09-28T10:00:00+08:00",
  }),
  row({
    key: "b",
    jobCode: "HY-L10",
    title: "Beyoncé Halo",
    client: { name: "Acme Music", code: "HY" },
    status: "done",
    fields: { notes: "Grade due 10 Oct" },
    createdAt: "2026-08-01T10:00:00+08:00",
  }),
  row({
    key: "c",
    jobCode: "OWN-L02",
    title: "아이유 좋은 날",
    space: "Personal",
    fields: { name: "좋은 날" },
    createdAt: "2026-09-01T10:00:00+08:00",
  }),
];

describe("search", () => {
  it("finds by code, title, client and any field value", () => {
    expect(matches(rows[0], "vx-l04")).toBe(true);
    expect(matches(rows[0], "summer")).toBe(true);
    expect(matches(rows[0], "vertex")).toBe(true);
    expect(matches(rows[0], "9x16")).toBe(true); // a multi-choice field
    expect(matches(rows[1], "oct")).toBe(true); // description
    expect(matches(rows[0], "acme music")).toBe(false);
  });

  it("finds a job by its artist and by who it's billed to", () => {
    const retainer = row({
      key: "r",
      jobCode: "VX-L03",
      title: "NOVA BAND Easy",
      client: { name: "Vertex Studio", code: "VX" }, // the billing party
      fields: { artist: "NOVA BAND", song: "Easy" }, // who the work is for
    });
    expect(matches(retainer, "nova")).toBe(true); // artist
    expect(matches(retainer, "vertex studio")).toBe(true); // billed to (name)
    expect(matches(retainer, "vx")).toBe(true); // billed to (code)
    expect(matches(retainer, "vx-l03")).toBe(true); // job code
    expect(matches(retainer, "nova vertex")).toBe(true); // both at once
  });

  it("ignores accents and case, keeps Korean", () => {
    expect(fold("Beyoncé")).toBe("beyonce");
    expect(matches(rows[1], "BEYONCE")).toBe(true);
    expect(matches(rows[2], "좋은")).toBe(true);
  });

  it("needs every word, in any order", () => {
    expect(matches(rows[0], "me kira")).toBe(true);
    expect(matches(rows[0], "kira halo")).toBe(false);
    expect(matches(rows[0], "   ")).toBe(true);
  });

  it("filters by space and status", () => {
    const keys = (f: Parameters<typeof filterProjects>[1]) => filterProjects(rows, f).map((r) => r.key);
    expect(keys({ space: null, status: "active", query: "" })).toEqual(["a", "c"]);
    expect(keys({ space: null, status: "done", query: "" })).toEqual(["b"]);
    expect(keys({ space: "Personal", status: "all", query: "" })).toEqual(["c"]);
    expect(keys({ space: "Work", status: "all", query: "halo" })).toEqual(["b"]);
  });

  it("sorts newest first by default and by code with numbers in order", () => {
    expect(sortProjects(rows, { key: "created", dir: "desc" }).map((r) => r.key)).toEqual(["a", "c", "b"]);
    const codes = [row({ key: "x", jobCode: "VX-L10" }), row({ key: "y", jobCode: "VX-L9" })];
    expect(sortProjects(codes, { key: "code", dir: "asc" }).map((r) => r.jobCode)).toEqual(["VX-L9", "VX-L10"]);
  });
});

describe("Devices", () => {
  const base = {
    key: "k",
    id: "k",
    jobCode: "ACME-W01",
    title: "T",
    client: { name: "Acme", code: "ACME" },
    space: "Work",
    status: "active",
    templateId: "video",
    templateName: "Video",
    createdAt: "",
    fields: null,
    error: null,
    paths: ["D:/Jobs/T"],
    offline: false,
    duplicateCode: false,
    archived: false,
  };
  const here = { ...base };
  const both = { ...base, key: "b", alsoOn: ["MacBook"] };
  const mac = {
    ...base,
    key: "remote:x:m",
    paths: [],
    device: { deviceId: "x", name: "MacBook", updatedAt: "2026-10-02T10:00:00+08:00", path: "/Users/me/T" },
  };

  it("filters by computer: this one, another one (incl. projects on both), or all", async () => {
    const { computerMatches } = await import("./search");
    expect([here, both, mac].filter((p) => computerMatches(p, "")).map((p) => p.key)).toEqual(["k", "b"]);
    expect([here, both, mac].filter((p) => computerMatches(p, "MacBook")).map((p) => p.key)).toEqual(["b", "remote:x:m"]);
    expect([here, both, mac].filter((p) => computerMatches(p, null))).toHaveLength(3);
  });

  it("says how long ago, and never a negative time", async () => {
    const { ago, isFresh } = await import("./search");
    const now = Date.parse("2026-10-02T12:00:00Z");
    expect(ago("2026-10-02T11:59:30Z", now)).toBe("just now");
    expect(ago("2026-10-02T11:55:00Z", now)).toBe("5 min ago");
    expect(ago("2026-10-02T09:00:00Z", now)).toBe("3 h ago");
    expect(ago("2026-09-30T12:00:00Z", now)).toBe("2 days ago");
    expect(ago("2026-10-02T12:10:00Z", now)).toBe("just now"); // the other clock is ahead
    expect(ago("", now)).toBe("");
    expect(isFresh("2026-10-02T00:00:00Z", now)).toBe(true);
    expect(isFresh("2026-09-30T00:00:00Z", now)).toBe(false);
  });

  it("formats bytes like Finder", async () => {
    const { formatBytes } = await import("./search");
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(999)).toBe("999 B");
    expect(formatBytes(1000)).toBe("1.0 KB");
    expect(formatBytes(2_500_000)).toBe("2.5 MB");
    expect(formatBytes(412_000_000_000)).toBe("412 GB");
    expect(formatBytes(1.5e12)).toBe("1.5 TB");
    expect(formatBytes(-1)).toBe("");
  });

  it("archived projects only show under Archived or All statuses, whatever their status", () => {
    const base = { space: null, query: "", computer: null };
    const active = row({ key: "a" });
    const done = row({ key: "d", status: "done" });
    const archivedDone = row({ key: "x", status: "done", archived: true });
    const archivedActive = row({ key: "y", archived: true });
    const all = [active, done, archivedDone, archivedActive];
    const keys = (status: "active" | "done" | "all" | "archived") =>
      filterProjects(all, { ...base, status }).map((p) => p.key);
    expect(keys("active")).toEqual(["a"]);
    expect(keys("done")).toEqual(["d"]);
    expect(keys("archived")).toEqual(["x", "y"]);
    expect(keys("all")).toEqual(["a", "d", "x", "y"]);
    // The "ready to archive" review shows only those projects.
    const ready = row({ key: "r", status: "done", readyToArchive: true });
    expect(filterProjects([...all, ready], { ...base, status: "done", ready: true }).map((p) => p.key)).toEqual(["r"]);
  });
});
