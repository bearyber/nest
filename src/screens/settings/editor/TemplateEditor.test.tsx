// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Template, TemplatePreview } from "../../../lib/types";

const save = vi.fn(async (...args: [string, Template, string]) => ({ key: "user:video", name: args[1].name }));
vi.mock("../../../lib/commands", () => ({
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
}));
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: vi.fn() } }));

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
  tree: ["01_BRIEF", { name: "04_GRAPHICS", when: "graphics" }],
  files: [{ from: "starter/NOTES.md", to: "NOTES.md", fill: true }],
};

afterEach(cleanup);

describe("TemplateEditor", () => {
  it("edits name, questions and folders, then saves the whole template", async () => {
    const saved = vi.fn();
    render(
      <TemplateEditor
        sourceKey="builtin:video"
        mode="customize"
        start={VIDEO}
        starterFiles={["starter/NOTES.md"]}
        step="questions"
        onClose={() => {}}
        onSaved={saved}
      />,
    );
    expect(screen.getByRole("dialog", { name: "Customize “Video”" })).toBeTruthy();
    // The example name and job code show on every step.
    expect((await screen.findAllByText("260302_SUMMER_CAMPAIGN")).length).toBeGreaterThan(0);
    expect(screen.getByText("ACME-W01")).toBeTruthy();
    // The template name sits above the steps.
    fireEvent.change(screen.getByLabelText("Template name"), { target: { value: "My video" } });

    // Step 1: questions, one line each.
    fireEvent.click(screen.getByRole("button", { name: /^1\s*Questions$/ }));
    // Billed to is locked: it makes the job code; No client is per project.
    expect((screen.getAllByLabelText("Question")[0] as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText(/tick “No client” in New Project/)).toBeTruthy();
    // Required is a switch on the line.
    const required = screen.getAllByRole("switch", { name: "Required" }) as HTMLInputElement[];
    expect(required[0].checked).toBe(true); // Project name
    fireEvent.click(screen.getByRole("button", { name: "+ Add a question ▾" }));
    await waitFor(() => expect(screen.getAllByLabelText("Question")).toHaveLength(5));
    const labels = screen.getAllByLabelText("Question") as HTMLInputElement[];
    fireEvent.change(labels[labels.length - 1], { target: { value: "Agency" } });
    // Details stay folded until ▸ is clicked.
    expect(screen.queryByText(/Ticked to start with/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "More about Has graphics" }));
    expect(screen.getByText(/Ticked to start with/)).toBeTruthy();
    // "Has graphics" decides a folder, so removing it asks first.
    fireEvent.click(screen.getByRole("button", { name: "Remove Has graphics" }));
    expect(screen.getByRole("alert").textContent).toContain("decides 1 folder");
    fireEvent.click(screen.getByRole("button", { name: "Keep it" }));

    // Step 2: the folder name, as blocks with a style each, and answers to click.
    fireEvent.click(screen.getByRole("button", { name: /^2\s*Folder name$/ }));
    expect(screen.getByLabelText("How Start date is written")).toBeTruthy();
    expect(screen.getByRole("button", { name: "+ Agency" })).toBeTruthy();
    expect(screen.queryByText("Title in Nest's project list")).toBeNull(); // under More options

    fireEvent.click(screen.getByRole("button", { name: /^3\s*Folders$/ }));
    expect(screen.getByText("if “Has graphics”")).toBeTruthy();
    // Nothing selected: the panel says what to do.
    expect(screen.getByText(/Click a folder on the left/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "+ Add folder" }));
    // The new folder is selected; its name is edited in the panel.
    fireEvent.change(screen.getByLabelText("Folder name"), { target: { value: "07_ADMIN" } });
    expect(screen.getByText("07_ADMIN")).toBeTruthy();
    // Choosing "only if not ticked" in the panel.
    fireEvent.click(screen.getByLabelText("Only if “Has graphics” is not ticked"));
    expect(screen.getByText("if not “Has graphics”")).toBeTruthy();

    const saveButton = screen.getByRole("button", { name: "Save template" }) as HTMLButtonElement;
    await waitFor(() => expect(saveButton.disabled).toBe(false));
    fireEvent.click(saveButton);
    await waitFor(() => expect(saved).toHaveBeenCalled());
    const [key, draft, mode] = save.mock.calls[0] as unknown as [string, Template, string];
    expect(key).toBe("builtin:video");
    expect(mode).toBe("customize");
    expect(draft.name).toBe("My video");
    expect(draft.fields.map((f) => f.label)).toEqual(["Billed to", "Project name", "Start date", "Has graphics", "Agency"]);
    expect(draft.fields[4].key).toBe("new_question"); // renaming never changes the key
    expect(draft.tree).toEqual(["01_BRIEF", { name: "04_GRAPHICS", when: "graphics" }, { name: "07_ADMIN", when: "!graphics" }]);
    expect(draft.files).toEqual(VIDEO.files);
  });

  it("asks before closing with unsaved changes", () => {
    const close = vi.fn();
    render(
      <TemplateEditor sourceKey="builtin:video" mode="customize" start={VIDEO} starterFiles={[]} step="name" onClose={close} onSaved={() => {}} />,
    );
    fireEvent.change(screen.getByLabelText("Template name"), { target: { value: "X" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(close).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Close without saving" }));
    expect(close).toHaveBeenCalled();
  });
});
