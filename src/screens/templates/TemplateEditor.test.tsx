// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Template, TemplatePreview } from "../../lib/types";

const save = vi.fn(async (...args: [string, Template, string]) => ({ key: "user:video", name: args[1].name }));
vi.mock("../../lib/commands", () => ({
  previewTemplate: vi.fn(
    async (): Promise<TemplatePreview> => ({
      folderName: "260302_SUMMER_CAMPAIGN",
      jobCode: "ACME-W01",
      title: "Project name",
      folders: ["01_BRIEF"],
      files: ["NOTES.md"],
      problems: [],
    }),
  ),
  saveTemplate: (key: string, draft: Template, mode: string) => save(key, draft, mode),
  editorAddFiles: vi.fn(),
}));
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: vi.fn() } }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));

import TemplateEditor from "./TemplateEditor";

const VIDEO: Template = {
  schema: 1,
  id: "video",
  version: 1,
  name: "Video",
  description: "Shoots and edits",
  fields: [
    { key: "client", label: "Billed to", type: "client", required: true },
    { key: "name", label: "Project name", type: "text", required: true },
    { key: "start", label: "Start date", type: "date", required: true, default: "today" },
    { key: "graphics", label: "Has graphics", type: "bool", default: false },
  ],
  folderName: "{start|yymmdd}_{name|caps}",
  tree: ["01_BRIEF", "02_FOOTAGE", "02_FOOTAGE/A_CAM", { name: "04_GRAPHICS", when: "graphics" }],
  files: [{ from: "starter/NOTES.md", to: "NOTES.md", fill: true }],
};

afterEach(() => {
  cleanup();
  save.mockClear();
});

const open = (onSaved = vi.fn(), onClose = vi.fn()) =>
  render(<TemplateEditor sourceKey="editor:1" mode="customize" start={VIDEO} onClose={onClose} onSaved={onSaved} />);

async function saveAndGet(): Promise<Template> {
  const button = screen.getByRole("button", { name: "Save template" }) as HTMLButtonElement;
  await waitFor(() => expect(button.disabled).toBe(false));
  fireEvent.click(button);
  await waitFor(() => expect(save).toHaveBeenCalled());
  return save.mock.calls[0][1];
}

describe("TemplateEditor (one screen)", () => {
  it("shows the whole template as a tree, blanks as chips, and what New Project asks", async () => {
    open();
    expect(screen.getByText("01_BRIEF")).toBeTruthy();
    expect(screen.getByText("A_CAM")).toBeTruthy();
    expect(screen.getByText("NOTES.md")).toBeTruthy();
    expect(screen.getByText("only if “Has graphics”")).toBeTruthy();
    // The project folder name: chips, with the real example next to it.
    expect(screen.getAllByRole("button", { name: "Start date" }).length).toBeGreaterThan(0);
    expect(await screen.findByText("260302_SUMMER_CAMPAIGN")).toBeTruthy();
    const asks = screen.getByLabelText("What New Project asks");
    expect(asks.textContent).toContain("Client");
    expect(asks.textContent).toContain("Has graphics");
  });

  it("adds a folder inside a folder, named straight away, and saves it", async () => {
    open();
    // "Add folder" inside 02_FOOTAGE (it's the second one: top level, 01_BRIEF, 02_FOOTAGE…).
    const adds = screen.getAllByText("+ Add folder");
    // 01_BRIEF's (empty), A_CAM's (empty), 02_FOOTAGE's, 04_GRAPHICS's, the project folder's.
    fireEvent.click(adds[2]);
    const box = await screen.findByLabelText("Folder name");
    fireEvent.change(box, { target: { value: "B_CAM" } });
    fireEvent.keyDown(box, { key: "Escape" }); // cancel: keeps "New folder"
    expect(screen.getByText("New folder")).toBeTruthy();
    fireEvent.doubleClick(screen.getByText("New folder"));
    const again = await screen.findByLabelText("Folder name");
    fireEvent.change(again, { target: { value: "B_CAM" } });
    fireEvent.keyDown(again, { key: "Enter" });
    // Enter also starts the next folder below: name it, then Tab moves it inside B_CAM.
    const next = await screen.findByLabelText("Folder name");
    fireEvent.change(next, { target: { value: "PROXIES" } });
    fireEvent.keyDown(next, { key: "Tab" });
    await act(async () => {});
    const box2 = await screen.findByLabelText("Folder name");
    fireEvent.keyDown(box2, { key: "Escape" });
    const t = await saveAndGet();
    expect(t.tree).toContainEqual("02_FOOTAGE/B_CAM/PROXIES");
    expect(t.tree).toContainEqual({ name: "04_GRAPHICS", when: "graphics" });
    expect(t.files).toEqual(VIDEO.files);
  });

  it("types a blank into a name with {", async () => {
    open();
    fireEvent.doubleClick(screen.getByText("01_BRIEF"));
    const box = await screen.findByLabelText("Folder name");
    fireEvent.change(box, { target: { value: "01_BRIEF_{" } });
    const find = await screen.findByLabelText("Find or name a blank");
    fireEvent.change(find, { target: { value: "proj" } });
    fireEvent.keyDown(find, { key: "Enter" });
    // A new blank, made by typing a name that doesn't exist yet.
    const after = screen.getAllByLabelText("Folder name, more")[0];
    fireEvent.change(after, { target: { value: "_{" } });
    const find2 = await screen.findByLabelText("Find or name a blank");
    fireEvent.change(find2, { target: { value: "Shoot day" } });
    fireEvent.click(screen.getByRole("option", { name: "Add “Shoot day” as a blank" }));
    fireEvent.keyDown(screen.getAllByLabelText("Folder name, more")[1], { key: "Escape" });
    // Escape cancels the whole rename: try again and commit with Enter instead.
    fireEvent.doubleClick(screen.getByText("01_BRIEF"));
    const box2 = await screen.findByLabelText("Folder name");
    fireEvent.change(box2, { target: { value: "01_{" } });
    const find3 = await screen.findByLabelText("Find or name a blank");
    fireEvent.change(find3, { target: { value: "Shoot" } });
    fireEvent.keyDown(find3, { key: "Enter" });
    fireEvent.keyDown(screen.getAllByLabelText("Folder name, more")[0], { key: "Enter" });
    await act(async () => {});
    const box3 = screen.queryByLabelText("Folder name");
    if (box3) fireEvent.keyDown(box3, { key: "Escape" });
    const t = await saveAndGet();
    expect(t.tree).toContainEqual("01_{shoot_day}");
    expect(t.fields.map((f) => f.label)).toContain("Shoot day");
  });

  it("deletes without asking, and Undo brings it back", async () => {
    open();
    fireEvent.click(screen.getByText("02_FOOTAGE"));
    const tree = screen.getByRole("tree").parentElement!;
    fireEvent.keyDown(tree, { key: "Delete" });
    expect(screen.queryByText("A_CAM")).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("Deleted 02_FOOTAGE");
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(screen.getByText("A_CAM")).toBeTruthy();
    // And with the keyboard: Ctrl/Cmd+Z.
    fireEvent.click(screen.getByText("01_BRIEF"));
    fireEvent.keyDown(tree, { key: "Delete" });
    expect(screen.queryByText("01_BRIEF")).toBeNull();
    fireEvent.keyDown(window, { key: "z", ctrlKey: true, metaKey: true });
    expect(screen.getByText("01_BRIEF")).toBeTruthy();
  });

  it("changes a blank's settings from the bottom line", () => {
    open();
    fireEvent.click(screen.getAllByRole("button", { name: "Has graphics" })[0]);
    fireEvent.change(screen.getByLabelText("Blank name"), { target: { value: "Graphics" } });
    expect(screen.getByText("only if “Graphics”")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove blank" }));
    // The folder stays, made always; Undo is offered.
    expect(screen.queryByText(/only if/)).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("Removed the blank");
  });

  it("asks before closing with unsaved changes", () => {
    const close = vi.fn();
    open(vi.fn(), close);
    fireEvent.change(screen.getByLabelText("Template name"), { target: { value: "X" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(close).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Close without saving" }));
    expect(close).toHaveBeenCalled();
  });
});
