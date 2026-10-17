// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ResolveProject, ResolveStatus, Template } from "../../lib/types";

const save = vi.fn(async (...args: [string, Template, string]) => ({ key: "user:video", name: args[1].name }));
const status = vi.fn(async (): Promise<ResolveStatus> => "ready");
const projects = vi.fn(
  async (): Promise<ResolveProject[]> => [
    { folder: "", name: "Base look" },
    { folder: "Clients/Acme", name: "Acme grade" },
    { folder: "Clients", name: "Old grade" },
  ],
);
const exportDrp = vi.fn(async (...args: [string, string, string]) => `files/${args[1]}.drp`);
const addDrp = vi.fn(async (_s: string, path: string) => `files/${path.split("/").pop()}`);
const addFiles = vi.fn(async () => ({ folders: [], files: [], skipped: [] }));

vi.mock("../../lib/commands", () => ({
  previewTemplate: vi.fn(async () => ({ folderName: "X", jobCode: "J", title: "T", folders: [], files: [], problems: [] })),
  exampleNames: vi.fn(async (_k: string, _d: Template, names: string[]) => names),
  saveTemplate: (key: string, draft: Template, mode: string) => save(key, draft, mode),
  editorAddFiles: (...a: unknown[]) => addFiles(...(a as [])),
  editorAddTextFile: vi.fn(),
  editorAddResolveDrp: (s: string, p: string) => addDrp(s, p),
  resolveStatus: () => status(),
  resolveProjects: () => projects(),
  resolveExportToEditor: (s: string, n: string, f: string) => exportDrp(s, n, f),
}));
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: async () => ({ popup: async () => {} }) } }));
type DragEvent = { payload: { type: string; paths?: string[]; position?: { x: number; y: number } } };
let onDrag: ((e: DragEvent) => void) | null = null;
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (fn: (e: DragEvent) => void) => {
      onDrag = fn;
      return () => {};
    },
  }),
}));

import { runEditStep } from "../../lib/editMenu";
import TemplateEditor from "./TemplateEditor";
import { pickRows } from "./ResolveSection";

const VIDEO: Template = {
  schema: 1,
  id: "video",
  version: 1,
  name: "Video",
  description: "",
  fields: [
    { key: "client", label: "Billed to", type: "client", required: true },
    { key: "name", label: "Project name", type: "text", required: true },
  ],
  folderName: "{client}_{name}",
  tree: ["01_BRIEF"],
  files: [],
};

beforeEach(() => {
  // jsdom has no layout: drops land on no folder in particular.
  document.elementFromPoint = () => null;
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  status.mockImplementation(async () => "ready");
  onDrag = null;
});

const open = (start: Template = VIDEO) =>
  render(<TemplateEditor sourceKey="editor:1" mode="customize" start={start} onClose={vi.fn()} onSaved={vi.fn()} />);
const section = () => screen.getByRole("region", { name: "Resolve project" });

async function saved(): Promise<Template> {
  const button = screen.getByRole("button", { name: "Save template" }) as HTMLButtonElement;
  await waitFor(() => expect(button.disabled).toBe(false));
  fireEvent.click(button);
  await waitFor(() => expect(save).toHaveBeenCalled());
  return save.mock.calls[0][1];
}

const WITH_RESOLVE: Template = { ...VIDEO, resolve: { drp: "files/Grade.drp", folder: "{client}", on: true } };

describe("Resolve project in the template editor", () => {
  it("shows a quiet empty state under the files", () => {
    open();
    const s = section();
    expect(within(s).getByText("Add a DaVinci Resolve starter project. New jobs get their own copy in Resolve.")).toBeTruthy();
    expect(within(s).getByRole("button", { name: "Choose from Resolve…" })).toBeTruthy();
    expect(within(s).getByText("or drop a .drp file")).toBeTruthy();
  });

  it("chooses a project from Resolve, with its folders, and exports it into the template", async () => {
    open();
    fireEvent.click(screen.getByRole("button", { name: "Choose from Resolve…" }));
    const list = await screen.findByRole("listbox", { name: "Projects in Resolve" });
    // Top-level projects first, then folders with what's in them.
    expect(list.textContent).toBe("Base lookClientsOld gradeAcmeAcme grade");
    fireEvent.click(within(list).getByRole("option", { name: "Acme grade" }));
    expect(exportDrp).toHaveBeenCalledWith("editor:1", "Acme grade", "Clients/Acme");
    await screen.findByText("Acme grade", { selector: "span" });
    expect(section().textContent).toContain("Resolve project: Acme grade");
    expect((await saved()).resolve).toEqual({ drp: "files/Acme grade.drp", folder: "{client}", on: true });
  });

  it("shows a spinner while Resolve exports", async () => {
    let finish: (v: string) => void = () => {};
    exportDrp.mockImplementationOnce(() => new Promise<string>((r) => (finish = r)));
    open();
    fireEvent.click(screen.getByRole("button", { name: "Choose from Resolve…" }));
    fireEvent.click(await screen.findByRole("option", { name: "Base look" }));
    expect(await within(section()).findByText("Getting “Base look” from Resolve…")).toBeTruthy();
    await act(async () => finish("files/Base look.drp"));
    expect(section().textContent).toContain("Resolve project: Base look");
  });

  it.each([
    ["notRunning", "Open DaVinci Resolve first."],
    [
      "scriptingOff",
      "In Resolve: Preferences → System → General → External scripting: Local, then restart Resolve. If Resolve has just opened, wait a few seconds.",
    ],
    ["notInstalled", "DaVinci Resolve isn't installed on this computer. You can still drop a .drp file."],
  ] as const)("says what to do when Resolve is %s", async (st, text) => {
    status.mockImplementation(async () => st);
    open();
    fireEvent.click(screen.getByRole("button", { name: "Choose from Resolve…" }));
    expect(await within(section()).findByText(text)).toBeTruthy();
    expect(projects).not.toHaveBeenCalled();
    expect(screen.queryByRole("listbox", { name: "Projects in Resolve" })).toBeNull();
  });

  it("puts it at the top of the library when New Project doesn't ask for a client", async () => {
    open({ ...VIDEO, fields: [VIDEO.fields[1]], folderName: "{name}" });
    fireEvent.click(screen.getByRole("button", { name: "Choose from Resolve…" }));
    fireEvent.click(await screen.findByRole("option", { name: "Base look" }));
    await waitFor(() => expect(section().textContent).toContain("Resolve project: Base look"));
    expect((await saved()).resolve).toEqual({ drp: "files/Base look.drp", folder: "", on: true });
  });

  it("takes a .drp dropped anywhere as the Resolve project; other files are added as before", async () => {
    open();
    await waitFor(() => expect(onDrag).not.toBeNull());
    await act(async () =>
      onDrag!({ payload: { type: "drop", paths: ["/Users/me/Look.drp", "/Users/me/notes.txt"], position: { x: 1, y: 1 } } }),
    );
    expect(addDrp).toHaveBeenCalledWith("editor:1", "/Users/me/Look.drp");
    expect(addFiles).toHaveBeenCalledWith("editor:1", "", ["/Users/me/notes.txt"]);
    await waitFor(() => expect(section().textContent).toContain("Resolve project: Look"));
    expect((await saved()).resolve).toEqual({ drp: "files/Look.drp", folder: "{client}", on: true });
  });

  it("removes it, and Undo brings it back", async () => {
    open(WITH_RESOLVE);
    expect(section().textContent).toContain("Resolve project: Grade");
    fireEvent.click(within(section()).getByRole("button", { name: "Remove" }));
    expect(within(section()).getByRole("button", { name: "Choose from Resolve…" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(section().textContent).toContain("Resolve project: Grade");
  });

  it("sets the name in Resolve, the folder and the tick, and Undo steps back through them", async () => {
    open(WITH_RESOLVE);
    // Empty name: the job folder's name.
    expect(section().textContent).toContain("same as the job folder");
    fireEvent.click(screen.getByRole("button", { name: "Name in Resolve" }));
    const name = within(screen.getByRole("group", { name: "Editing: Name in Resolve" })).getByLabelText("Name in Resolve");
    fireEvent.change(name, { target: { value: "Grade" } });
    fireEvent.keyDown(name, { key: "Enter" });

    fireEvent.click(screen.getByRole("button", { name: "Put it in folder" }));
    const folder = within(screen.getByRole("group", { name: "Editing: Put it in folder" })).getByLabelText("Put it in folder");
    // Slashes are folders inside folders here.
    fireEvent.change(folder, { target: { value: "Jobs/" } });
    fireEvent.keyDown(folder, { key: "Enter" });
    expect(screen.getByText("Use / for folders inside folders. Leave empty for none.")).toBeTruthy();

    fireEvent.click(screen.getByRole("checkbox", { name: "Ticked in New Project" }));

    (document.activeElement as HTMLElement | null)?.blur();
    act(() => runEditStep("undo"));
    expect((screen.getByRole("checkbox", { name: "Ticked in New Project" }) as HTMLInputElement).checked).toBe(true);
    act(() => runEditStep("undo"));
    act(() => runEditStep("redo"));
    act(() => runEditStep("redo"));
    const t = await saved();
    expect(t.resolve).toEqual({ drp: "files/Grade.drp", name: "Grade", folder: "Jobs/{client}", on: false });
  });
});

describe("pickRows", () => {
  it("lists folders once, nested, with projects under them, and filters by name", () => {
    const rows = pickRows(
      [
        { folder: "B", name: "b1" },
        { folder: "A/X", name: "x1" },
        { folder: "", name: "top" },
        { folder: "A", name: "a1" },
      ],
      "",
    );
    expect(rows.map((r) => `${r.depth}:${r.kind === "folder" ? `[${r.name}]` : r.project.name}`)).toEqual([
      "0:top",
      "0:[A]",
      "1:a1",
      "1:[X]",
      "2:x1",
      "0:[B]",
      "1:b1",
    ]);
    expect(pickRows([{ folder: "A/X", name: "x1" }, { folder: "", name: "top" }], "X1").map((r) => r.key)).toEqual([
      "folder:A",
      "folder:A/X",
      "project:A/X/x1",
    ]);
  });
});
