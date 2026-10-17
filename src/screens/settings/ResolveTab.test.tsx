// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ResolveWaitingView, SettingsView, TemplateInfo } from "../../lib/types";

const EMPTY: ResolveWaitingView = { entries: [], lastStatus: null, hint: null };
const ENTRY = {
  id: "p1",
  jobCode: "VX-L01",
  projectDir: "/jobs/VX-L01_KIRA",
  drp: "/config/resolve/p1.drp",
  name: "KIRA",
  folder: "Vertex Studio",
  library: null,
  createdAt: "2026-10-17T10:00:00+02:00",
  tries: 0,
  lastOutcome: "notRunning",
  lastTry: null,
};

const template = (resolve: boolean): TemplateInfo =>
  ({ key: "user:g", id: "g", name: "Grade", resolve, superseded: false }) as TemplateInfo;

vi.mock("../../lib/commands", () => ({
  listTemplates: vi.fn(async () => [template(true)]),
  resolveStatus: vi.fn(async () => "ready"),
  resolveLibraries: vi.fn(async () => ({
    current: { dbType: "Disk", dbName: "Local Database" },
    libraries: [
      { dbType: "Disk", dbName: "Local Database" },
      { dbType: "Disk", dbName: "Work 2026" },
    ],
  })),
  resolveWaitingList: vi.fn(async () => EMPTY),
  resolveTryNow: vi.fn(async () => EMPTY),
  resolveRemoveWaiting: vi.fn(async () => EMPTY),
  resolveSetLibrary: vi.fn(async () => ({})),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import * as commands from "../../lib/commands";
import ResolveTab from "./ResolveTab";

function view(resolveLibrary: { dbType: string; dbName: string } | null = null): SettingsView {
  return { settings: { resolveLibrary } } as unknown as SettingsView;
}
const run = <T,>(p: Promise<T>) => p;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Settings → DaVinci Resolve", () => {
  it("one line when no template uses a Resolve project", async () => {
    vi.mocked(commands.listTemplates).mockResolvedValueOnce([template(false)]);
    render(<ResolveTab view={view()} run={run} />);
    expect(await screen.findByText("No template uses a Resolve project yet.")).toBeTruthy();
    expect(screen.queryByText("Add projects to")).toBeNull();
    expect(commands.resolveStatus).not.toHaveBeenCalled();
  });

  it("ready: ✓, libraries to choose from, and the hint", async () => {
    render(<ResolveTab view={view()} run={run} />);
    expect(await screen.findByText("✓ Resolve is ready")).toBeTruthy();
    const select = screen.getByLabelText("Add projects to") as HTMLSelectElement;
    await waitFor(() => expect(select.options.length).toBe(3));
    expect(Array.from(select.options).map((o) => o.text)).toEqual([
      "The library open in Resolve",
      "Local Database",
      "Work 2026",
    ]);
    expect(select.value).toBe("");
    expect(screen.getByText("Nest only switches library when no project is open in Resolve.")).toBeTruthy();
    expect(screen.getByText("Nothing is waiting.")).toBeTruthy();

    fireEvent.change(select, { target: { value: "Disk\nWork 2026" } });
    expect(commands.resolveSetLibrary).toHaveBeenCalledWith({ dbType: "Disk", dbName: "Work 2026" });
    fireEvent.change(select, { target: { value: "" } });
    expect(commands.resolveSetLibrary).toHaveBeenLastCalledWith(null);
  });

  it("scripting off: the hint allows for Resolve just opening; the saved library stays a choice", async () => {
    vi.mocked(commands.resolveStatus).mockResolvedValueOnce("scriptingOff");
    render(<ResolveTab view={view({ dbType: "Disk", dbName: "Work 2026" })} run={run} />);
    expect(await screen.findByText(/External scripting using.*Local\. If Resolve has just opened/)).toBeTruthy();
    expect(commands.resolveLibraries).not.toHaveBeenCalled();
    const select = screen.getByLabelText("Add projects to") as HTMLSelectElement;
    expect(select.value).toBe("Disk\nWork 2026");
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => expect(commands.resolveStatus).toHaveBeenCalledTimes(2));
  });

  it("not open / not installed", async () => {
    vi.mocked(commands.resolveStatus).mockResolvedValueOnce("notRunning");
    render(<ResolveTab view={view()} run={run} />);
    expect(await screen.findByText(/^Open Resolve to check/)).toBeTruthy();
    cleanup();
    vi.mocked(commands.resolveStatus).mockResolvedValueOnce("notInstalled");
    render(<ResolveTab view={view()} run={run} />);
    expect(await screen.findByText("DaVinci Resolve isn't installed on this computer.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Check again" })).toBeNull();
  });

  it("the waiting list: name, since, last result; Try now and Remove", async () => {
    const list: ResolveWaitingView = { entries: [ENTRY], lastStatus: "notRunning", hint: "Open DaVinci Resolve first." };
    vi.mocked(commands.resolveWaitingList).mockResolvedValue(list);
    vi.mocked(commands.resolveTryNow).mockResolvedValueOnce(list);
    render(<ResolveTab view={view()} run={run} />);
    expect(await screen.findByText("KIRA")).toBeTruthy();
    expect(screen.getByText("VX-L01")).toBeTruthy();
    expect(screen.getByText(/^Since .*17.* · Resolve wasn't open$/)).toBeTruthy();
    expect(screen.getByText("Open DaVinci Resolve first.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Try now" }));
    await waitFor(() => expect(commands.resolveTryNow).toHaveBeenCalled());
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    await waitFor(() => expect(commands.resolveRemoveWaiting).toHaveBeenCalledWith("p1"));
    expect(await screen.findByText("Nothing is waiting.")).toBeTruthy();
    vi.mocked(commands.resolveWaitingList).mockResolvedValue(EMPTY);
  });

  it("projects still waiting show even when no template uses Resolve any more", async () => {
    vi.mocked(commands.listTemplates).mockResolvedValueOnce([]);
    vi.mocked(commands.resolveWaitingList).mockResolvedValueOnce({ entries: [ENTRY], lastStatus: null, hint: null });
    render(<ResolveTab view={view()} run={run} />);
    expect(await screen.findByText("KIRA")).toBeTruthy();
    expect(screen.queryByText("No template uses a Resolve project yet.")).toBeNull();
  });
});
