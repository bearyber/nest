// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
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
const show = (
  resolve: ProjectResolve | null,
  onResolveTryNow?: () => Promise<void>,
  onResolveSkip?: () => Promise<void>,
) =>
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
      onResolveTryNow={onResolveTryNow}
      onResolveSkip={onResolveSkip}
      sizes={null}
      measuring={false}
      onMeasure={noop}
      onOpenFolder={noop}
      onMeasureFolder={() => Promise.reject(new Error("no"))}
    />,
  );

afterEach(cleanup);

describe("Inspector: Resolve project", () => {
  const r: ProjectResolve = {
    name: "KIRA",
    folder: "Vertex Studio",
    library: null,
    status: "waiting",
    at: "",
    note: null,
    waiting: false,
    problem: null,
  };

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

  it("while it waits here: Try now and Don't add, and what stops it on the same line", async () => {
    let finish: () => void = () => {};
    const tryNow = vi.fn(() => new Promise<void>((done) => (finish = done)));
    const skip = vi.fn(async () => {});
    const problem = "Turn on External scripting in Resolve: Preferences → System → General → Local.";
    show({ ...r, waiting: true, problem }, tryNow, skip);
    expect(screen.getByText(`· ${problem}`)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Try now" }));
    expect(tryNow).toHaveBeenCalledTimes(1);
    // Busy while it tries: neither button again.
    expect((screen.getByRole("button", { name: "Trying…" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Don't add" }) as HTMLButtonElement).disabled).toBe(true);
    await act(async () => finish());
    fireEvent.click(screen.getByRole("button", { name: "Don't add" }));
    expect(skip).toHaveBeenCalledTimes(1);
  });

  it("no buttons when it isn't waiting on this computer; a note shows after the status", () => {
    show({ ...r, waiting: false });
    expect(screen.queryByRole("button", { name: "Try now" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Don't add" })).toBeNull();
    cleanup();
    const note = "“Client Work” isn't on this computer, so it went into “Local Database”.";
    show({ ...r, status: "added", at: "", note });
    expect(screen.getByText("Resolve project: KIRA · added")).toBeTruthy();
    expect(screen.getByText(`· ${note}`)).toBeTruthy();
    cleanup();
    show({ ...r, status: "skipped" });
    expect(screen.getByText(/not added \(taken off the waiting list\)/)).toBeTruthy();
  });

  it("shows nothing without one", () => {
    show(null);
    expect(screen.queryByText(/Resolve project/)).toBeNull();
  });
});
