// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Created, NewProjectContext, Plan, PlanRequest, ProjectList, ProjectRow } from "../lib/types";

// ── Stand-ins for Rust and Tauri ──

let projects: ProjectRow[] = [];
let tipSeen = false;

const row = (code: string, title: string): ProjectRow => ({
  key: `id-${code}`,
  id: `id-${code}`,
  jobCode: code,
  title,
  client: { name: "Acme", code: "ACME" },
  space: "Work",
  status: "active",
  templateId: "general",
  createdAt: "2026-10-17T10:00:00Z",
  fields: null,
  error: null,
  paths: [`/Users/me/Jobs/${code}_${title}`],
  offline: false,
  duplicateCode: false,
  templateName: "General project",
  archived: false,
});

const CREATED: Created = {
  key: "id-ACME-L01",
  jobCode: "ACME-L01",
  title: "Summer Film",
  root: "/Users/me/Documents/Work/Clients/Jobs/261017_SUMMER FILM",
  bumped: false,
};

const context: NewProjectContext = {
  templates: [
    {
      schema: 1,
      id: "general",
      version: 2,
      name: "General project",
      description: "",
      folderName: "{start|yymmdd}_{name|caps}",
      fields: [
        { key: "client", label: "Billed to", type: "client", required: true },
        { key: "name", label: "Project name", type: "text", required: true, max: 60 },
        { key: "start", label: "Start date", type: "date", required: true, default: "today" },
        { key: "notes", label: "Description", type: "longtext" },
      ],
      tree: ["01_BRIEF", "02_REFERENCE"],
    },
    {
      schema: 1,
      id: "video",
      version: 1,
      name: "Video",
      description: "",
      folderName: "{name}",
      fields: [{ key: "name", label: "Project name", type: "text", required: true }],
      tree: ["FOOTAGE"],
    },
  ],
  spaces: ["Work", "Personal"],
  noClientSpaces: ["Personal"],
  defaultSpace: "Work",
  // The last template used is not the first: the example still uses the first.
  lastTemplate: "video",
  jobsRoot: "/Users/me/Jobs",
  rootMissing: false,
  personalRoot: null,
  personalMissing: false,
  clients: [],
  settingsError: null,
};

vi.mock("../lib/commands", () => ({
  listProjects: vi.fn(async (): Promise<ProjectList> => ({ projects, roots: [], scannedAt: 1 })),
  rescanProjects: vi.fn(async () => ({ found: projects.length, removed: 0, duplicates: [], offline: [] })),
  devicesView: vi.fn(async () => ({ enabled: false, others: [] })),
  devicesRefresh: vi.fn(async () => {}),
  getSettings: vi.fn(async () => ({
    settings: { spaces: ["Work", "Personal"], archiveAfterDays: null, archiveFolder: null, firstProjectTipSeen: tipSeen },
  })),
  takeNotice: vi.fn(async () => null),
  whatsNew: vi.fn(async () => null),
  whatsNewSeen: vi.fn(async () => {}),
  pendingUpdate: vi.fn(async () => null),
  checkForUpdates: vi.fn(async () => null),
  installUpdate: vi.fn(async () => {}),
  firstProjectTipSeen: vi.fn(async () => {}),
  revealProject: vi.fn(async () => {}),
  openProject: vi.fn(async () => {}),
  openSettings: vi.fn(async () => {}),
  projectSizes: vi.fn(async () => null),
  measureProject: vi.fn(async () => null),
  measureProjectFolder: vi.fn(async () => null),
  openProjectSubfolder: vi.fn(async () => {}),
  copyText: vi.fn(async () => {}),
  archiveProject: vi.fn(async () => null),
  dismissArchiveSuggestion: vi.fn(async () => {}),
  requestStatus: vi.fn(async () => {}),
  setStatus: vi.fn(async () => {}),
  // New Project
  newProjectContext: vi.fn(async () => context),
  planProject: vi.fn(async (req: PlanRequest): Promise<Plan> => {
    const name = typeof req.values.name === "string" ? req.values.name : "";
    return {
      jobCode: "ACME-L01",
      folderName: `261017_${name.toUpperCase()}`,
      title: name,
      root: `/Users/me/Jobs/261017_${name.toUpperCase()}`,
      folders: ["01_BRIEF", "02_REFERENCE"],
      files: [],
      manifest: {},
      fill: {},
      issues: name ? [] : [{ level: "error", field: "name", message: "Project name is required" }],
    };
  }),
  createProject: vi.fn(async () => CREATED),
  cancelCreate: vi.fn(async () => {}),
  chooseJobsRoot: vi.fn(async () => null),
  choosePersonalRoot: vi.fn(async () => null),
  chooseClientKit: vi.fn(async () => null),
  setTemplateKitFolder: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "0.7.0") }));
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: vi.fn() } }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onFocusChanged: vi.fn(async () => () => {}) }),
}));
vi.mock("../screens/templates/Templates", () => ({ default: () => null }));

import * as commands from "../lib/commands";
import Main from "../screens/Main";
import { exampleValues } from "../screens/NewProjectSheet";
import FirstSteps, { FirstProjectNote } from "./FirstSteps";

beforeEach(() => {
  // jsdom has no scrolling; the list keeps the selected row in view.
  Element.prototype.scrollIntoView = vi.fn();
  projects = [];
  tipSeen = false;
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  sessionStorage.clear();
});

/** The New Project sheet's Create button (its label also holds the shortcut). */
const createButton = () => screen.getByRole("button", { name: /^Create/ }) as HTMLButtonElement;

describe("FirstSteps", () => {
  it("shows three steps and two buttons", () => {
    const start = vi.fn();
    const example = vi.fn();
    render(<FirstSteps onStart={start} onExample={example} />);
    expect(screen.getByText("Make your first project", { selector: "h2" })).toBeTruthy();
    expect(screen.getByText("Three steps, about a minute.")).toBeTruthy();
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "1. Answer a few questionsClient, project name, date.",
      "2. Nest makes the foldersNamed the same way, every time.",
      "3. Find it here laterSearch, status, sizes.",
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Make your first project" }));
    fireEvent.click(screen.getByRole("button", { name: "Try an example" }));
    expect(start).toHaveBeenCalledOnce();
    expect(example).toHaveBeenCalledOnce();
  });

  it("the note says where the folder is, in plain words", () => {
    const reveal = vi.fn();
    const done = vi.fn();
    render(<FirstProjectNote code="ACME-L01" folder="/…/Clients/Jobs" onReveal={reveal} onDone={done} />);
    const text = document.body.textContent ?? "";
    expect(text).toContain("ACME-L01 is ready");
    expect(text).toContain("It's a normal folder in /…/Clients/Jobs. Put your files in it, Nest keeps track.");
    expect(text).not.toMatch(/successfully|!/i);
    fireEvent.click(screen.getByRole("button", { name: /^Show in (Finder|Explorer)$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(reveal).toHaveBeenCalledOnce();
    expect(done).toHaveBeenCalledOnce();
  });
});

describe("exampleValues", () => {
  it("fills client Acme, the project name and today's date, keeping other defaults", () => {
    const v = exampleValues(context.templates[0].fields, [], "2026-10-17");
    expect(v).toEqual({ client: { name: "Acme", code: "ACME" }, name: "Summer Film", start: "2026-10-17" });
  });

  it("keeps the code of an Acme already in the jobs folder", () => {
    const v = exampleValues(context.templates[0].fields, [{ name: "ACME", code: "AC" }], "2026-10-17");
    expect(v.client).toEqual({ name: "Acme", code: "AC" });
  });

  it("uses the first required text field when there's no name field", () => {
    const v = exampleValues([
      { key: "artist", label: "Artist", type: "text", required: true },
      { key: "song", label: "Song", type: "text" },
    ]);
    expect(v).toEqual({ artist: "Summer Film" });
  });
});

describe("Main: first steps", () => {
  it("shows the guide when there are no projects at all", async () => {
    render(<Main />);
    expect(await screen.findByText("Three steps, about a minute.")).toBeTruthy();
    expect(screen.queryByText("No projects yet")).toBeNull();
  });

  it("doesn't show the guide when there are projects, even when the filter is empty", async () => {
    projects = [{ ...row("ACME-L01", "Summer Film"), status: "done" }];
    render(<Main />);
    // Active is the default filter: nothing to show, but not the guide.
    expect(await screen.findByText("No active projects")).toBeTruthy();
    expect(screen.queryByText("Three steps, about a minute.")).toBeNull();
  });

  it("an empty search still shows the search empty state", async () => {
    projects = [row("ACME-L01", "Summer Film")];
    render(<Main />);
    await screen.findByText("Summer Film");
    fireEvent.change(screen.getByLabelText("Search projects"), { target: { value: "zzz" } });
    expect(await screen.findByText("No matches for “zzz”")).toBeTruthy();
    expect(screen.queryByText("Three steps, about a minute.")).toBeNull();
  });

  it("Try an example opens New Project filled in, and creates nothing", async () => {
    render(<Main />);
    fireEvent.click(await screen.findByRole("button", { name: "Try an example" }));
    expect(await screen.findByText(/Nothing is made until you press Create/)).toBeTruthy();
    // The first template (not the last one used), filled in.
    await waitFor(() => expect((screen.getByPlaceholderText("Name") as HTMLInputElement).value).toBe("Acme"));
    expect((screen.getByLabelText("Client code") as HTMLInputElement).value).toBe("ACME");
    expect((screen.getByLabelText(/^Project name/) as HTMLInputElement).value).toBe("Summer Film");
    // The preview shows what would be made.
    await waitFor(() => expect(document.body.textContent).toContain("261017_SUMMER FILM"));
    expect(screen.getByText("01_BRIEF")).toBeTruthy();
    expect(vi.mocked(commands.planProject).mock.lastCall?.[0].templateId).toBe("general");
    // Nothing made: Cancel closes it.
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(commands.createProject).not.toHaveBeenCalled();
    expect(commands.firstProjectTipSeen).not.toHaveBeenCalled();
  });

  it("New Project from the guide starts empty (not the example)", async () => {
    render(<Main />);
    fireEvent.click(await screen.findByRole("button", { name: "Make your first project" }));
    await screen.findByRole("dialog");
    await waitFor(() => expect(screen.getByLabelText(/^Project name/)).toBeTruthy());
    expect(screen.queryByText(/Nothing is made until you press Create/)).toBeNull();
    expect((screen.getByLabelText(/^Project name/) as HTMLInputElement).value).toBe("");
  });

  it("after the very first project, the note shows once and not after Got it", async () => {
    render(<Main />);
    fireEvent.click(await screen.findByRole("button", { name: "Try an example" }));
    await waitFor(() => expect(createButton().disabled).toBe(false));
    projects = [row("ACME-L01", "Summer Film")]; // what the list holds after Create
    fireEvent.click(createButton());
    expect(await screen.findByText("ACME-L01 is ready")).toBeTruthy();
    expect(document.body.textContent).toContain("It's a normal folder in /…/Clients/Jobs.");
    expect(commands.createProject).toHaveBeenCalledOnce();
    expect(commands.firstProjectTipSeen).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: /^Show in (Finder|Explorer)$/ }));
    expect(commands.revealProject).toHaveBeenCalledWith("id-ACME-L01");
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(screen.queryByText("ACME-L01 is ready")).toBeNull();

    // A second project (the last template used, Video): the usual toast, no note.
    fireEvent.click(screen.getByRole("button", { name: /New/ }));
    await screen.findByRole("dialog");
    fireEvent.change(await screen.findByLabelText(/^Project name/), { target: { value: "Winter" } });
    await waitFor(() => expect(createButton().disabled).toBe(false));
    await act(async () => {
      fireEvent.click(createButton());
    });
    await waitFor(() => expect(commands.createProject).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("Created ACME-L01")).toBeTruthy();
    expect(screen.queryByText("ACME-L01 is ready")).toBeNull();
    expect(commands.firstProjectTipSeen).toHaveBeenCalledOnce();
  });

  it("never shows the note once Settings says it was seen", async () => {
    tipSeen = true;
    render(<Main />);
    fireEvent.click(await screen.findByRole("button", { name: "Try an example" }));
    await waitFor(() => expect(createButton().disabled).toBe(false));
    fireEvent.click(createButton());
    expect(await screen.findByText("Created ACME-L01")).toBeTruthy();
    expect(screen.queryByText("ACME-L01 is ready")).toBeNull();
  });
});
