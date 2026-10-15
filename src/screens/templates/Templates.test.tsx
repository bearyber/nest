// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Saved, TemplateInfo } from "../../lib/types";

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
vi.mock("../../lib/commands", () => ({
  listTemplates: vi.fn(async () => templates),
  editorOpen: (key: string) => editorOpen(key),
  editorClose: vi.fn(async () => {}),
  exportTemplate: vi.fn(async () => true),
  importTemplate: vi.fn(async () => null),
  removeTemplate: (key: string) => removeTemplate(key),
  restoreBuiltinTemplates: () => restoreBuiltinTemplates(),
  setTemplateHidden: vi.fn(async () => ({})),
  templateFromFolder: (path: string | null) => templateFromFolder(path),
}));

type MenuItem = { id?: string; text?: string; action?: () => void; items?: MenuItem[] };
let menuItems: MenuItem[] = [];
vi.mock("@tauri-apps/api/menu", () => ({
  Menu: {
    new: vi.fn(async (opts: { items: MenuItem[] }) => {
      menuItems = opts.items;
      return { popup: async () => {} };
    }),
  },
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
    await act(async () => menuItems.find((i) => i.id === "remove")?.action?.());
    expect(removeTemplate).toHaveBeenCalledWith("user:acme-film");

    menuItems = [];
    fireEvent.contextMenu(screen.getByText("Video"));
    await waitFor(() => expect(menuItems.length).toBeGreaterThan(0));
    await act(async () => menuItems.find((i) => i.id === "remove")?.action?.());
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
});
