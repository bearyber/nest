// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import type { ProjectResolve, ProjectRow } from "../lib/types";
import Inspector from "./Inspector";

const ROW = {
  key: "p1",
  id: "p1",
  jobCode: "VX-L01",
  title: "KIRA",
  client: { name: "Vertex Studio", code: "VX" },
  space: "Work",
  status: "active",
  templateId: "g",
  templateName: "Grade",
  createdAt: "2026-10-17T10:00:00+02:00",
  fields: {},
  error: null,
  paths: ["/jobs/VX-L01_KIRA"],
  offline: false,
  duplicateCode: false,
  archived: false,
} as ProjectRow;

const noop = () => {};
const show = (resolve: ProjectResolve | null) =>
  render(
    <Inspector
      project={ROW}
      onOpen={noop}
      onCopyPath={noop}
      onStatus={noop}
      archiveOn={false}
      onArchive={noop}
      onDismissArchive={noop}
      resolve={resolve}
      sizes={null}
      measuring={false}
      onMeasure={noop}
      onOpenFolder={noop}
      onMeasureFolder={() => Promise.reject(new Error("no"))}
    />,
  );

afterEach(cleanup);

describe("Inspector: Resolve project", () => {
  const r: ProjectResolve = { name: "KIRA", folder: "Vertex Studio", library: null, status: "waiting", at: "" };

  it("says where the project's Resolve project is", () => {
    show(r);
    expect(screen.getByText("Resolve project: KIRA · waiting for Resolve")).toBeTruthy();
    cleanup();
    show({ ...r, status: "exists" });
    expect(screen.getByText("Resolve project: KIRA · already in Resolve")).toBeTruthy();
    cleanup();
    show({ ...r, status: "added", at: "2026-10-17T10:00:00+02:00" });
    expect(screen.getByText(/^Resolve project: KIRA · added .*17/)).toBeTruthy();
  });

  it("shows nothing without one", () => {
    show(null);
    expect(screen.queryByText(/Resolve project/)).toBeNull();
  });
});
