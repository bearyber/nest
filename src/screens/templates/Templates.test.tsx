// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { DropKind, Saved, TemplateInfo } from "../../lib/types";

const info = (over: Partial<TemplateInfo>): TemplateInfo => ({
  key: "builtin:video",
  id: "video",
  name: "Video",
  version: 1,
  description: "",
  builtIn: true,
  superseded: false,
  replaced: false,
  hidden: false,
  folder: "",
  folders: 3,
  ...over,
});

let templates: TemplateInfo[] = [];
const editorOpen = vi.fn(async (key: string) => ({
  key: "editor:1",
  draft: { template: { name: key, tree: [] }, builtIn: false, replaced: false, starterFiles: [] },
}));
const removeTemplate = vi.fn<(key: string) => Promise<boolean>>(async () => true);
const restoreBuiltinTemplates = vi.fn(async () => ({}));
const templateFromFolder = vi.fn<(path: string | null) => Promise<null>>(async () => null);
const importTemplate = vi.fn<() => Promise<TemplateInfo | null>>(async () => null);
const importTemplateFrom = vi.fn<(path: string) => Promise<TemplateInfo | null>>(async (path) =>
  info({ key: `user:${path}`, name: path.split("/").pop()!.replace(/\.\w+$/, ""), builtIn: false }),
);
/** By name, like the Rust side would see on disk: folders have no dot, templates end in .nesttemplate/.json. */
const dropKinds = vi.fn(async (paths: string[]): Promise<DropKind[]> =>
  paths.map((p) => (/\.(nesttemplate|json)$/i.test(p) ? "template" : /\.\w+$/.test(p) ? "other" : "folder")),
);
vi.mock("../../lib/commands", () => ({
  dropKinds: (paths: string[]) => dropKinds(paths),
  importTemplateFrom: (path: string) => importTemplateFrom(path),
  listTemplates: vi.fn(async () => templates),
  editorOpen: (key: string) => editorOpen(key),
  editorClose: vi.fn(async () => {}),
  exportTemplate: vi.fn(async () => true),
  importTemplate: () => importTemplate(),
  removeTemplate: (key: string) => removeTemplate(key),
  restoreBuiltinTemplates: () => restoreBuiltinTemplates(),
  setTemplateHidden: vi.fn(async () => ({})),
  templateFromFolder: (path: string | null) => templateFromFolder(path),
}));

type MenuItem = { id?: string; text?: string; action?: () => void; items?: MenuItem[] };
let menuItems: MenuItem[] = [];
vi.mock("../../lib/popupMenu", () => ({
  popupMenu: vi.fn(async (items: MenuItem[]) => {
    menuItems = items;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
type DropEvent = { payload: { type: string; paths: string[] } };
let onDrop: ((e: DropEvent) => void) | null = null;
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (h: (e: DropEvent) => void) => {
      onDrop = h;
      return () => {};
    },
  }),
}));
// The editor itself has its own tests: here it only shows how it was opened, and can save.
vi.mock("./TemplateEditor", () => ({
  default: (p: { mode: string; onSaved: (s: Saved) => void }) => (
    <div data-testid="editor" data-mode={p.mode}>
      <button
        type="button"
        onClick={() => p.onSaved({ info: info({ key: "user:video", builtIn: false }), binned: 0, binProblem: null })}
      >
        Save
      </button>
    </div>
  ),
}));

import Templates from "./Templates";

const runMock = vi.fn<(p: Promise<unknown>, ok?: string) => Promise<unknown>>((p) => p.catch(() => undefined));
const run = runMock as unknown as <T>(p: Promise<T>, ok?: string) => Promise<T | undefined>;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  menuItems = [];
});

const NEST = info({});
const YOURS = info({ key: "user:acme-film", id: "acme-film", name: "Acme film", builtIn: false, folders: 1 });

describe("Templates", () => {
  it("lists each template once: yours replace Nest's, removed ones of Nest's are left out", async () => {
    templates = [
      info({ key: "builtin:photo", id: "photo", name: "Photo", superseded: true, replaced: true }),
      info({ key: "user:photo", id: "photo", name: "Photo", builtIn: false }),
      info({ key: "user:photo-old", id: "photo", name: "Photo", builtIn: false, superseded: true }),
      info({ key: "builtin:design", id: "design", name: "Design", hidden: true }),
      NEST,
      YOURS,
    ];
    render(<Templates run={run} />);
    await screen.findByText("Acme film");
    expect(screen.getAllByText("Photo")).toHaveLength(1);
    expect(screen.queryByText("Design")).toBeNull();
    expect(screen.getByText("Nest's · 3 folders")).toBeTruthy();
    expect(screen.getByText("Yours · 1 folder")).toBeTruthy();
  });

  it("clicking one of Nest's opens the editor to customize it, and says the original is kept", async () => {
    templates = [NEST, YOURS];
    render(<Templates run={run} />);
    fireEvent.click(await screen.findByText("Video"));
    const editor = await screen.findByTestId("editor");
    expect(editorOpen).toHaveBeenCalledWith("builtin:video");
    expect(editor.dataset.mode).toBe("customize");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(runMock).toHaveBeenCalledWith(expect.anything(), "Saved as yours. Nest's original is kept."),
    );
  });

  it("clicking yours opens the editor to edit it", async () => {
    templates = [NEST, YOURS];
    render(<Templates run={run} />);
    fireEvent.click(await screen.findByText("Acme film"));
    const editor = await screen.findByTestId("editor");
    expect(editorOpen).toHaveBeenCalledWith("user:acme-film");
    expect(editor.dataset.mode).toBe("edit");
  });

  it("Remove on yours and on one of Nest's goes through remove_template", async () => {
    templates = [NEST, YOURS];
    render(<Templates run={run} />);
    fireEvent.click(await screen.findByRole("button", { name: "More for Acme film" }));
    await waitFor(() => expect(menuItems.length).toBeGreaterThan(0));
    const items = menuItems.map((i) => i.text);
    expect(items).toEqual(expect.arrayContaining(["New project from this", "Duplicate", "Export…", "Remove…"]));
    // No fixed ids: a shared id let an old right-click menu's clean-up take this one's actions away.
    expect(menuItems.some((i) => "id" in i)).toBe(false);
    await act(async () => menuItems.find((i) => i.text?.startsWith("Remove"))?.action?.());
    expect(removeTemplate).toHaveBeenCalledWith("user:acme-film");

    menuItems = [];
    fireEvent.contextMenu(screen.getByText("Video"));
    await waitFor(() => expect(menuItems.length).toBeGreaterThan(0));
    await act(async () => menuItems.find((i) => i.text?.startsWith("Remove"))?.action?.());
    expect(removeTemplate).toHaveBeenCalledWith("builtin:video");
  });

  it("brings back Nest's templates when any are removed", async () => {
    templates = [NEST, YOURS];
    const { unmount } = render(<Templates run={run} />);
    await screen.findByText("Video");
    expect(screen.queryByRole("button", { name: "Bring back Nest's templates" })).toBeNull();
    unmount();

    templates = [info({ hidden: true }), YOURS];
    render(<Templates run={run} />);
    await screen.findByText("Acme film");
    expect(screen.queryByText("Video")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Bring back Nest's templates" }));
    await waitFor(() => expect(restoreBuiltinTemplates).toHaveBeenCalled());
  });

  it("Choose a folder… and a dropped folder both make a template from the folder", async () => {
    templates = [NEST];
    render(<Templates run={run} />);
    fireEvent.click(await screen.findByRole("button", { name: "Choose a folder…" }));
    await waitFor(() => expect(templateFromFolder).toHaveBeenCalledWith(null));

    await waitFor(() => expect(onDrop).not.toBeNull());
    await act(async () => onDrop?.({ payload: { type: "drop", paths: ["/Jobs/260301_ACME"] } }));
    expect(templateFromFolder).toHaveBeenCalledWith("/Jobs/260301_ACME");
  });

  it("New template offers Start empty and a copy of any template", async () => {
    templates = [info({ key: "builtin:blank", id: "blank", name: "Blank" }), NEST, YOURS];
    render(<Templates run={run} />);
    fireEvent.click(await screen.findByRole("button", { name: "New template" }));
    await waitFor(() => expect(menuItems.length).toBeGreaterThan(0));
    expect(menuItems.map((i) => i.text)).toEqual(["Start empty", "From a copy of…"]);
    expect(menuItems[1].items?.map((i) => i.text)).toEqual(["Video", "Acme film"]);
    await act(async () => menuItems[0].action?.());
    expect(editorOpen).toHaveBeenCalledWith("builtin:blank");
    expect((await screen.findByTestId("editor")).dataset.mode).toBe("new");
  });

  it("a dropped template file is installed like Import…, with the same notice", async () => {
    templates = [NEST];
    render(<Templates run={run} />);
    await screen.findByText("Video");
    await waitFor(() => expect(onDrop).not.toBeNull());
    await act(async () => onDrop?.({ payload: { type: "drop", paths: ["/Downloads/Acme film.nesttemplate"] } }));
    expect(importTemplateFrom).toHaveBeenCalledWith("/Downloads/Acme film.nesttemplate");
    expect(templateFromFolder).not.toHaveBeenCalled();
    await waitFor(() => expect(runMock).toHaveBeenCalledWith(expect.anything(), "Added “Acme film”."));
  });

  it("a mixed drop installs every template file and uses only the first folder", async () => {
    templates = [NEST];
    render(<Templates run={run} />);
    await screen.findByText("Video");
    await waitFor(() => expect(onDrop).not.toBeNull());
    await act(async () =>
      onDrop?.({
        payload: {
          type: "drop",
          paths: ["/Jobs/260301_ACME", "/Downloads/A.nesttemplate", "/Jobs/260302_OTHER", "/Downloads/B.json"],
        },
      }),
    );
    expect(importTemplateFrom.mock.calls.map((c) => c[0])).toEqual(["/Downloads/A.nesttemplate", "/Downloads/B.json"]);
    expect(templateFromFolder).toHaveBeenCalledTimes(1);
    expect(templateFromFolder).toHaveBeenCalledWith("/Jobs/260301_ACME");
    expect(runMock).toHaveBeenCalledWith(
      expect.anything(),
      "Added “A” and “B”. Used the first folder only; left out “260302_OTHER”.",
    );
  });

  it("a file that's neither a folder nor a template says so and does nothing", async () => {
    templates = [NEST];
    render(<Templates run={run} />);
    await screen.findByText("Video");
    await waitFor(() => expect(onDrop).not.toBeNull());
    await act(async () => onDrop?.({ payload: { type: "drop", paths: ["/Desktop/notes.txt"] } }));
    expect(importTemplateFrom).not.toHaveBeenCalled();
    expect(templateFromFolder).not.toHaveBeenCalled();
    const shown = runMock.mock.calls[runMock.mock.calls.length - 1][0] as Promise<unknown>;
    await expect(shown).rejects.toThrow("That's not a folder or a Nest template file.");
  });

  it("“or open a template file…” opens the same picker as Import…", async () => {
    templates = [NEST];
    render(<Templates run={run} />);
    fireEvent.click(await screen.findByRole("button", { name: "or open a template file…" }));
    await waitFor(() => expect(importTemplate).toHaveBeenCalledTimes(1));
    expect(screen.getByText("Drop a job folder or a template file here")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Import…" })).toBeTruthy();
  });
});
