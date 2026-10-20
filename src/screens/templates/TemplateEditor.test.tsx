// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { Template, TemplatePreview } from "../../lib/types";

const save = vi.fn(async (...args: [string, Template, string]) => ({ key: "user:video", name: args[1].name }));
/** A stand-in for Rust's example names: the project folder, and answers typed into names. */
const exampleNames = vi.fn(async (_key: string, _draft: Template, names: string[]) =>
  names.map((n) =>
    n === "{start|yymmdd}_{name|caps}"
      ? "260302_SUMMER_CAMPAIGN"
      : n.replace(/\{name\}/g, "SummerCampaign").replace(/\{client\}/g, "Acme").replace(/\{start\|yymmdd\}/g, "260302"),
  ),
);
type MenuOpts = { items: { text?: string; action?: () => void }[] };
const menuNew = vi.fn(async (opts?: MenuOpts) => ({ popup: async () => {}, opts }));
const addText = vi.fn(async (...[, into, name]: [string, string, string, string]) => ({
  folders: [],
  files: [{ from: `files/${name}`, to: into ? `${into}/${name}` : name, fill: true }],
  skipped: [],
}));
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
  exampleNames: (key: string, draft: Template, names: string[]) => exampleNames(key, draft, names),
  saveTemplate: (key: string, draft: Template, mode: string) => save(key, draft, mode),
  editorAddFiles: vi.fn(),
  editorAddTextFile: (key: string, into: string, name: string, contents: string) => addText(key, into, name, contents),
}));
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: (opts: MenuOpts) => menuNew(opts) } }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));

import { runEditStep } from "../../lib/editMenu";
import TemplateEditor from "./TemplateEditor";
import WhatsNew from "../../components/WhatsNew";

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
  exampleNames.mockClear();
  menuNew.mockClear();
});

const open = (onSaved = vi.fn(), onClose = vi.fn()) =>
  render(<TemplateEditor sourceKey="editor:1" mode="customize" start={VIDEO} onClose={onClose} onSaved={onSaved} />);

/** Open, and wait until the names show as the example project has them. */
async function openWithExamples() {
  open();
  await screen.findByText("260302_SUMMER_CAMPAIGN");
}

const tree = () => screen.getByRole("tree");
const row = (text: string) => screen.getByText(text).closest('[role="treeitem"]') as HTMLElement;

async function saveAndGet(): Promise<Template> {
  const button = screen.getByRole("button", { name: "Save template" }) as HTMLButtonElement;
  await waitFor(() => expect(button.disabled).toBe(false));
  fireEvent.click(button);
  await waitFor(() => expect(save).toHaveBeenCalled());
  return save.mock.calls[0][1];
}

/** Commit a folder name with Enter, then let go of the new folder Enter starts below it. */
async function enterAndStop(input: HTMLElement) {
  fireEvent.keyDown(input, { key: "Enter" });
  await act(async () => {});
  const next = screen.queryByLabelText("Folder name");
  if (next) fireEvent.keyDown(next, { key: "Escape" });
}

describe("TemplateEditor (one screen)", () => {
  it("shows the whole template as a tree, and what New Project asks", async () => {
    open();
    expect(screen.getByText("A new project gets these folders. Click a name to change it.")).toBeTruthy();
    expect(screen.getByText("01_BRIEF")).toBeTruthy();
    expect(screen.getByText("A_CAM")).toBeTruthy();
    expect(screen.getByText("NOTES.md")).toBeTruthy();
    expect(screen.getByText("only if “Has graphics”")).toBeTruthy();
    const asks = screen.getByLabelText("What New Project asks");
    expect(asks.textContent).toBe("New Project will ask for:Client,Project name,Start date,Has graphicsAdd a question");
  });

  it("shows names as an example project until you click one to change it", async () => {
    await openWithExamples();
    // One call with every name the tree shows.
    const names = exampleNames.mock.calls[exampleNames.mock.calls.length - 1][2];
    expect(names).toEqual(expect.arrayContaining(["{start|yymmdd}_{name|caps}", "01_BRIEF", "A_CAM", "NOTES.md"]));
    // No chips in the tree while not renaming.
    expect(within(tree()).queryByRole("button", { name: "Start date" })).toBeNull();
    // Click the name: it's the pattern again, with chips.
    fireEvent.click(screen.getByText("260302_SUMMER_CAMPAIGN"));
    const field = await screen.findByLabelText("Editing: Project folder name");
    expect(within(field).getByRole("button", { name: "Start date" })).toBeTruthy();
    expect(within(field).getByRole("button", { name: "Project name" })).toBeTruthy();
    expect(screen.queryByText("260302_SUMMER_CAMPAIGN")).toBeNull();
  });

  it("puts New Project's answers in with the buttons under the name", async () => {
    await openWithExamples();
    fireEvent.click(screen.getByText("01_BRIEF"));
    await screen.findByLabelText("Folder name");
    // The template's own text, date and client questions, then More….
    const line = screen.getByText("Fill in from New Project:").parentElement!;
    expect(within(line).getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Client",
      "Project name",
      "Start date",
      "More…",
    ]);
    fireEvent.click(within(line).getByRole("button", { name: "Project name" }));
    fireEvent.click(within(line).getByRole("button", { name: "Start date" }));
    // More… opens the full list.
    fireEvent.click(within(line).getByRole("button", { name: "More…" }));
    expect(await screen.findByLabelText("Find an answer, or name a new question")).toBeTruthy();
    fireEvent.keyDown(screen.getByLabelText("Find an answer, or name a new question"), { key: "Escape" });

    await enterAndStop(screen.getAllByLabelText("Folder name, more").slice(-1)[0]);
    // Shown as the example project has it.
    expect(await screen.findByText("01_BRIEFSummerCampaign260302")).toBeTruthy();
    const t = await saveAndGet();
    expect(t.tree).toContainEqual("01_BRIEF{name}{start|yymmdd}");
  });

  it("offers New Project's answer for selected text", async () => {
    await openWithExamples();
    fireEvent.click(screen.getByText("01_BRIEF"));
    const box = (await screen.findByLabelText("Folder name")) as HTMLInputElement;
    expect(screen.queryByRole("button", { name: "Use New Project's answer" })).toBeNull();
    box.focus();
    box.setSelectionRange(3, 8);
    fireEvent.keyUp(box, { key: "ArrowLeft", shiftKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "Use New Project's answer" }));
    // The picker opens with the selected text, ready to become a new question.
    const find = (await screen.findByLabelText("Find an answer, or name a new question")) as HTMLInputElement;
    expect(find.value).toBe("BRIEF");
    fireEvent.click(screen.getByRole("option", { name: "Add “BRIEF” as a question" }));
    await enterAndStop(screen.getAllByLabelText("Folder name, more").slice(-1)[0]);
    const t = await saveAndGet();
    expect(t.fields.map((f) => f.label)).toContain("BRIEF");
    const key = t.fields.find((f) => f.label === "BRIEF")!.key;
    expect(t.tree).toContainEqual(`01_{${key}}`);
  });

  it("never says “blank”", async () => {
    await openWithExamples();
    fireEvent.click(screen.getByRole("button", { name: "Has graphics" }));
    fireEvent.click(screen.getByText("01_BRIEF"));
    await screen.findByLabelText("Folder name");
    fireEvent.click(within(screen.getByText("Fill in from New Project:").parentElement!).getByRole("button", { name: "More…" }));
    const find = await screen.findByLabelText("Find an answer, or name a new question");
    fireEvent.change(find, { target: { value: "Shoot day" } });
    expect(screen.getByRole("option", { name: "Add “Shoot day” as a question" })).toBeTruthy();
    const words = [document.body.textContent ?? ""];
    document.body.querySelectorAll("*").forEach((el) => {
      for (const a of ["aria-label", "placeholder", "title"]) words.push(el.getAttribute(a) ?? "");
    });
    expect(words.join(" ")).not.toMatch(/blank/i);
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

  it("types an answer into a name with {", async () => {
    open();
    fireEvent.doubleClick(screen.getByText("01_BRIEF"));
    const box = await screen.findByLabelText("Folder name");
    fireEvent.change(box, { target: { value: "01_BRIEF_{" } });
    const find = await screen.findByLabelText("Find an answer, or name a new question");
    fireEvent.change(find, { target: { value: "proj" } });
    fireEvent.keyDown(find, { key: "Enter" });
    // A new question, made by typing a name that doesn't exist yet.
    const after = screen.getAllByLabelText("Folder name, more")[0];
    fireEvent.change(after, { target: { value: "_{" } });
    const find2 = await screen.findByLabelText("Find an answer, or name a new question");
    fireEvent.change(find2, { target: { value: "Shoot day" } });
    fireEvent.click(screen.getByRole("option", { name: "Add “Shoot day” as a question" }));
    fireEvent.keyDown(screen.getAllByLabelText("Folder name, more")[1], { key: "Escape" });
    // Escape cancels the whole rename: try again and commit with Enter instead.
    fireEvent.doubleClick(screen.getByText("01_BRIEF"));
    const box2 = await screen.findByLabelText("Folder name");
    fireEvent.change(box2, { target: { value: "01_{" } });
    const find3 = await screen.findByLabelText("Find an answer, or name a new question");
    fireEvent.change(find3, { target: { value: "Shoot" } });
    fireEvent.keyDown(find3, { key: "Enter" });
    await enterAndStop(screen.getAllByLabelText("Folder name, more")[0]);
    const t = await saveAndGet();
    expect(t.tree).toContainEqual("01_{shoot_day}");
    expect(t.fields.map((f) => f.label)).toContain("Shoot day");
  });

  it("deletes without asking, and Undo brings it back", async () => {
    open();
    fireEvent.click(row("02_FOOTAGE"));
    const treeBox = tree().parentElement!;
    fireEvent.keyDown(treeBox, { key: "Delete" });
    expect(screen.queryByText("A_CAM")).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("Deleted 02_FOOTAGE");
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(screen.getByText("A_CAM")).toBeTruthy();
    // And with the keyboard: Ctrl+Z (Windows; on a Mac the menu bar's Undo).
    fireEvent.click(row("01_BRIEF"));
    fireEvent.keyDown(treeBox, { key: "Delete" });
    expect(screen.queryByText("01_BRIEF")).toBeNull();
    // Extra modifiers aren't Undo.
    fireEvent.keyDown(window, { key: "z", ctrlKey: true, altKey: true });
    expect(screen.queryByText("01_BRIEF")).toBeNull();
    fireEvent.keyDown(window, { key: "z", ctrlKey: true });
    expect(screen.getByText("01_BRIEF")).toBeTruthy();
  });

  it("opens the right-click menu from ⋯", async () => {
    open();
    fireEvent.click(screen.getByRole("button", { name: "More for 01_BRIEF" }));
    await waitFor(() => expect(menuNew).toHaveBeenCalled());
    expect(row("01_BRIEF").getAttribute("aria-selected")).toBe("true");
  });

  it("changes a question's settings from the bottom line", () => {
    open();
    fireEvent.click(screen.getByRole("button", { name: "Has graphics" }));
    expect(screen.getByLabelText("Question: Has graphics")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Question"), { target: { value: "Graphics" } });
    expect(screen.getByText("only if “Graphics”")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove question" }));
    // The folder stays, made always; Undo is offered.
    expect(screen.queryByText(/only if/)).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("Removed the question");
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
  describe("Escape never throws changes away", () => {
    const esc = (target: Element | Window = window) => fireEvent.keyDown(target, { key: "Escape" });

    it("closes straight away with nothing changed", () => {
      const close = vi.fn();
      open(vi.fn(), close);
      esc();
      expect(close).toHaveBeenCalledTimes(1);
    });

    it("with changes it asks; Escape again cancels the question and stays open", () => {
      const close = vi.fn();
      open(vi.fn(), close);
      fireEvent.change(screen.getByLabelText("Template name"), { target: { value: "X" } });
      esc();
      expect(screen.getByText(/Close without saving\?/)).toBeTruthy();
      esc();
      expect(screen.queryByText(/Close without saving\?/)).toBeNull();
      esc();
      esc();
      esc();
      // Even with a button focused (Escape there isn't a click).
      const button = screen.getByRole("button", { name: "Close without saving" });
      button.focus();
      esc(button);
      expect(close).not.toHaveBeenCalled();
      expect(screen.queryByText(/Close without saving\?/)).toBeNull();
      expect(screen.getByRole("dialog", { name: /Customize/ })).toBeTruthy();
      // Only the button closes without saving.
      fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
      fireEvent.click(screen.getByRole("button", { name: "Close without saving" }));
      expect(close).toHaveBeenCalledTimes(1);
    });

    it("closes an open question's settings first, and only that", () => {
      const close = vi.fn();
      open(vi.fn(), close);
      fireEvent.click(screen.getByRole("button", { name: "Has graphics" }));
      expect(screen.getByLabelText("Question: Has graphics")).toBeTruthy();
      esc();
      expect(screen.queryByLabelText("Question: Has graphics")).toBeNull();
      expect(close).not.toHaveBeenCalled();
    });

    it("leaves an Escape alone that something inside already used (a picker closing)", () => {
      const close = vi.fn();
      open(vi.fn(), close);
      const e = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
      e.preventDefault();
      act(() => void window.dispatchEvent(e));
      expect(close).not.toHaveBeenCalled();
    });

    it("with a dialog over the editor (What's new, New Project), Escape closes only that one", () => {
      const close = vi.fn();
      const ok = vi.fn();
      open(vi.fn(), close);
      const { unmount } = render(<WhatsNew note={{ version: "9.9.9", lines: ["A line"] }} onOk={ok} />);
      esc();
      expect(ok).toHaveBeenCalledTimes(1);
      expect(close).not.toHaveBeenCalled();
      unmount();
      esc();
      expect(close).toHaveBeenCalledTimes(1);
    });
  });

  it("adds a file from the Add a file menu: Project info with the template's answers", async () => {
    open();
    const links = screen.getAllByRole("button", { name: "+ Add a file" });
    fireEvent.click(links[links.length - 1]); // the one always shown, for the project folder
    await waitFor(() => expect(menuNew).toHaveBeenCalled());
    const items = (menuNew.mock.calls[menuNew.mock.calls.length - 1][0] as MenuOpts).items;
    expect(items.some((x) => x.text === "Your own file…")).toBe(true);
    await act(async () => items.find((x) => x.text?.startsWith("Project info"))!.action!());
    expect(addText).toHaveBeenCalled();
    const [, into, name, contents] = addText.mock.calls[0];
    expect(into).toBe("");
    expect(name).toBe("Project info.txt");
    expect(contents).toContain("Job code: {jobCode}");
    expect(contents).toContain("Client: {client}");
    expect(contents).toContain("Project name: {name}");
    expect(await screen.findByText("Project info.txt")).toBeTruthy();
  });

  describe("the order New Project asks the questions in", () => {
    const realFromPoint = document.elementFromPoint;
    afterEach(() => {
      document.elementFromPoint = realFromPoint;
    });
    const line = () => screen.getByLabelText("What New Project asks");
    const askButton = (name: string) => within(line()).getByRole("button", { name });
    const ask = (name: string) => askButton(name).closest("[data-ask-key]") as HTMLElement;
    const asked = () =>
      screen
        .getByLabelText("What New Project asks")
        .textContent!.replace("New Project will ask for:", "")
        .replace("Add a question", "");

    /** Drag a name along the bottom line onto the left or right half of another. */
    function drag(name: string, onto: string, side: "left" | "right") {
      const from = ask(name);
      const to = ask(onto);
      to.getBoundingClientRect = () => ({
        left: 100,
        width: 100,
        top: 0,
        height: 20,
        right: 200,
        bottom: 20,
        x: 100,
        y: 0,
        toJSON: () => ({}),
      });
      document.elementFromPoint = () => to.firstElementChild ?? to;
      const x = side === "left" ? 110 : 190;
      fireEvent.pointerDown(within(from).getByRole("button"), {
        button: 0,
        clientX: 0,
        clientY: 10,
        pointerId: 1,
      });
      fireEvent.pointerMove(from, {
        buttons: 1,
        clientX: x,
        clientY: 10,
        pointerId: 1,
      });
      fireEvent.pointerUp(from, { clientX: x, clientY: 10, pointerId: 1 });
    }

    it("drag a name to ask it earlier; it's saved in that order", async () => {
      open();
      drag("Has graphics", "Project name", "left");
      expect(asked()).toBe("Client,Has graphics,Project name,Start date");
      const saved = await saveAndGet();
      expect(saved.fields.map((f) => f.key)).toEqual(["client", "graphics", "name", "start"]);
    });

    it("keeps the client question first", () => {
      open();
      // The client question can't be picked up.
      expect(ask("Client").querySelector("[aria-hidden]")).toBeNull();
      drag("Client", "Start date", "right");
      expect(asked()).toBe("Client,Project name,Start date,Has graphics");
      // Nothing goes before it...
      drag("Start date", "Client", "left");
      expect(asked()).toBe("Client,Project name,Start date,Has graphics");
      // ...but right after it is fine.
      drag("Start date", "Client", "right");
      expect(asked()).toBe("Client,Start date,Project name,Has graphics");
    });

    it("a click without moving still opens the question", () => {
      open();
      fireEvent.pointerDown(askButton("Start date"), {
        button: 0,
        clientX: 5,
        clientY: 5,
      });
      fireEvent.pointerUp(ask("Start date"), { clientX: 5, clientY: 5 });
      fireEvent.click(askButton("Start date"));
      expect(screen.getByLabelText("Question: Start date")).toBeTruthy();
    });

    it("one reorder is one undo step", () => {
      open();
      drag("Has graphics", "Project name", "left");
      drag("Start date", "Client", "right");
      expect(asked()).toBe("Client,Start date,Has graphics,Project name");
      fireEvent.keyDown(window, { key: "z", ctrlKey: true });
      expect(asked()).toBe("Client,Has graphics,Project name,Start date");
      fireEvent.keyDown(window, { key: "z", ctrlKey: true });
      expect(asked()).toBe("Client,Project name,Start date,Has graphics");
    });
  });

  describe("Edit → Undo / Redo in the macOS menu bar", () => {
    const realExec = document.execCommand;
    afterEach(() => {
      document.execCommand = realExec;
    });

    it("steps the editor's history when the focus isn't in a text box", () => {
      open();
      fireEvent.click(row("01_BRIEF"));
      fireEvent.keyDown(tree().parentElement!, { key: "Delete" });
      expect(screen.queryByText("01_BRIEF")).toBeNull();
      (document.activeElement as HTMLElement | null)?.blur();
      act(() => runEditStep("undo"));
      expect(screen.getByText("01_BRIEF")).toBeTruthy();
      act(() => runEditStep("redo"));
      expect(screen.queryByText("01_BRIEF")).toBeNull();
    });

    it("in a text box it's the text's own undo, and the editor's history stays", () => {
      const exec = vi.fn(() => true);
      document.execCommand = exec;
      open();
      fireEvent.click(row("01_BRIEF"));
      fireEvent.keyDown(tree().parentElement!, { key: "Delete" });
      expect(screen.queryByText("01_BRIEF")).toBeNull();
      screen.getByLabelText("Template name").focus();
      act(() => runEditStep("undo"));
      expect(exec).toHaveBeenCalledWith("undo");
      expect(screen.queryByText("01_BRIEF")).toBeNull();
    });
  });
});
