// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { NewProjectContext, Plan, PlanRequest } from "../lib/types";

// Stand-in for Rust: artist is required, and the plan says so until it's filled.
vi.mock("../lib/commands", () => ({
  newProjectContext: vi.fn(
    async (): Promise<NewProjectContext> => ({
      templates: [
        {
          schema: 1,
          id: "grading-mv",
          version: 1,
          name: "Grading: MV",
          description: "",
          folderName: "{jobCode}_{artist}",
          fields: [
            { key: "client", label: "Client", type: "client", required: true },
            { key: "artist", label: "Artist", type: "text", required: true },
            { key: "ratios", label: "Delivery ratios", type: "multi", options: ["16x9", "9x16"], default: ["16x9"] },
            { key: "lookdev", label: "Include look dev", type: "bool", default: false },
          ],
          tree: [
            "01_REF",
            "02_BRAND/LOGOS",
            { name: "03_LOOKDEV", when: "lookdev" },
            { name: "04_DELIVERY/{item}", each: "ratios" },
          ],
        },
      ],
      spaces: ["Work", "Personal"],
      noClientSpaces: ["Personal"],
      defaultSpace: "Work",
      // Points at a template that no longer exists (removed built-in): falls back to the first.
      lastTemplate: "grading-performance",
      jobsRoot: "D:/NestDev",
      rootMissing: false,
      personalRoot: "D:/Personal",
      personalMissing: false,
      clients: [{ name: "Vertex Studio", code: "VX" }],
      settingsError: null,
    }),
  ),
  planProject: vi.fn(async (req: PlanRequest): Promise<Plan> => {
    const artist = typeof req.values.artist === "string" ? req.values.artist : "";
    // No client: every question is optional and the code is your own.
    const noClient = req.noClient || req.space === "Personal";
    const code = noClient ? "OWN-L01" : "VX-L01";
    const folderName = noClient ? `${code}${artist && `_${artist}`}` : `${code}_${artist || "Artist"}`;
    return {
      jobCode: code,
      folderName,
      title: artist,
      // Personal / No client projects go to the Personal folder (as Rust decides).
      root: `${noClient ? "D:/Personal" : "D:/NestDev"}/${folderName}`,
      folders: ["01_REF"],
      files: [],
      manifest: {},
      fill: {},
      issues: artist || noClient ? [] : [{ level: "error", field: "artist", message: "Artist is required" }],
    };
  }),
  createProject: vi.fn(),
  chooseJobsRoot: vi.fn(async () => null),
  choosePersonalRoot: vi.fn(async () => null),
  chooseClientKit: vi.fn(async () => null),
  setTemplateKitFolder: vi.fn(async () => {}),
  cancelCreate: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import * as commands from "../lib/commands";
import type { Field, Issue, PlanKit, TreeEntry } from "../lib/types";
import NewProjectSheet, { clientFilePlaces, shortPath } from "./NewProjectSheet";
import WhatsNew from "../components/WhatsNew";

/** Plans like the stand-in above, plus these changes. Returns a way to go back. */
function planWith(extra: Partial<Plan>) {
  const plan = vi.mocked(commands.planProject);
  const base = plan.getMockImplementation()!;
  plan.mockImplementation(async (req) => {
    const p = await base(req);
    return { ...p, ...extra, issues: [...p.issues, ...(extra.issues ?? [])] };
  });
  return () => plan.mockImplementation(base);
}

const withKit = (kit: PlanKit) => planWith({ kit });

const KIT: PlanKit = {
  state: "ready",
  client: "Vertex Studio",
  code: "VX",
  source: "/Users/me/Library/CloudStorage/GoogleDrive-me@acme.com/My Drive/Kits/VX",
  folder: "02_BRAND",
  count: 38,
  bytes: 1_200_000_000,
  fingerprint: "38-1200000000-abc",
  sample: ["02_BRAND/logo.png"],
};

const NONE: PlanKit = { ...KIT, state: "missing", source: undefined, fingerprint: undefined, count: 0, bytes: 0, sample: [] };

/** Everything the sheet shows as one string (lines made of several pieces read as a whole). */
const shown = () => document.body.textContent ?? "";

afterEach(() => {
  cleanup();
  sessionStorage.clear();
  vi.clearAllMocks();
});

describe("NewProjectSheet", () => {
  it("example: fills in Acme and the first text field, says nothing is made yet, creates nothing", async () => {
    render(<NewProjectSheet example onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByText(/Nothing is made until you press Create/)).toBeTruthy();
    await waitFor(() => expect((screen.getByPlaceholderText("Name") as HTMLInputElement).value).toBe("Acme"));
    expect((screen.getByLabelText("Client code") as HTMLInputElement).value).toBe("ACME");
    // No "name" field in this template: the first required text field gets the example name.
    expect((screen.getByLabelText(/^Artist/) as HTMLInputElement).value).toBe("Summer Film");
    // Template defaults stay (16x9 on).
    expect(screen.getByRole("button", { name: "16x9" }).getAttribute("aria-pressed")).toBe("true");
    await waitFor(() => expect(shown()).toContain("VX-L01_Summer Film"));
    expect(commands.createProject).not.toHaveBeenCalled();
  });

  it("Escape cancels it, but only when it's the top dialog", async () => {
    const close = vi.fn();
    render(<NewProjectSheet onClose={close} onCreated={() => {}} />);
    expect(await screen.findByLabelText(/^Artist/)).toBeTruthy();
    const over = render(<WhatsNew note={{ version: "9.9.9", lines: ["A line"] }} onOk={() => {}} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(close).not.toHaveBeenCalled();
    over.unmount();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("no example note normally", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByLabelText(/^Artist/)).toBeTruthy();
    expect(screen.queryByText(/Nothing is made until you press Create/)).toBeNull();
  });

  it("renders fields from the template", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    // lastTemplate names a removed template, so the first one is picked and its fields show.
    expect(await screen.findByLabelText(/^Artist/)).toBeTruthy();
    expect(screen.getByPlaceholderText("Name")).toBeTruthy();
    // The sheet always calls the paying party "Client", whatever the template says.
    expect(screen.getByText("Client")).toBeTruthy();
    expect(screen.queryByText("Billed to")).toBeNull();
    expect(screen.getByLabelText("Client code")).toBeTruthy();
    const lookdev = screen.getByLabelText("03_LOOKDEV folder") as HTMLInputElement;
    expect(lookdev.type).toBe("checkbox");
    expect(lookdev.checked).toBe(false);
    // Ratios are toggle chips; the template default (16x9) starts on.
    const on = screen.getByRole("button", { name: "16x9" });
    const off = screen.getByRole("button", { name: "9x16" });
    expect(on.getAttribute("aria-pressed")).toBe("true");
    expect(off.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(off);
    expect(off.getAttribute("aria-pressed")).toBe("true");
  });

  it("sums up where it goes in one line", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    await waitFor(() => expect(shown()).toContain("Goes in D:/NestDev · Work · VX-L01"));
    // The controls stay out of the way until Change…
    expect(screen.queryByRole("button", { name: "Work" })).toBeNull();
    expect(screen.queryByText("Folder name")).toBeNull();
    expect(screen.queryByLabelText("Edit job code")).toBeNull();
  });

  it("Change… shows the location, space, folder name and job code, and stays open", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    const change = await screen.findByRole("button", { name: "Change…" });
    expect(change.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(change);
    // Location keeps its own Change… (the folder picker); Space is a segmented control.
    expect(screen.getByText("Location")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Change…" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Work" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("Folder name")).toBeTruthy();
    expect(screen.getByLabelText("Edit job code")).toBeTruthy();
    const done = screen.getByRole("button", { name: "Done" });
    expect(done.getAttribute("aria-expanded")).toBe("true");

    // Remembered for the session: the next New Project opens with the controls showing.
    cleanup();
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByRole("button", { name: "Done" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Work" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("button", { name: "Work" })).toBeNull();
  });

  it("shows a job code error without Change… and opens the controls", async () => {
    const issue: Issue = { level: "error", field: "jobCode", message: 'Job code "VX-L01" is already used' };
    const restore = planWith({ issues: [issue] });
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByText('Job code "VX-L01" is already used')).toBeTruthy();
    // Open by itself, with nothing to close while the error stands.
    expect(screen.getByRole("button", { name: "Work" })).toBeTruthy();
    expect(screen.getByLabelText("Edit job code")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Done" })).toBeNull();
    restore();
  });

  it("a typed code with another computer's letter warns in plain words and never blocks Create", async () => {
    const message =
      'VX-W07 uses the letter W, which your computer "Studio PC" uses, so it could clash with a code made there. You can still use it.';
    const restore = planWith({ issues: [{ level: "warning", field: "jobCode", message }] });
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByText(message)).toBeTruthy();
    fireEvent.change(await screen.findByLabelText(/^Artist/), { target: { value: "KIRA" } });
    await screen.findAllByText("VX-L01_KIRA");
    const create = screen.getByRole("button", { name: /^Create/ }) as HTMLButtonElement;
    await waitFor(() => expect(create.disabled).toBe(false));
    restore();
  });

  it("keeps Create disabled until required fields are valid", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    const create = await screen.findByRole("button", { name: /^Create/ });
    // The preview tree's root.
    await screen.findAllByText("VX-L01_Artist");
    expect((create as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("Artist is required")).toBeTruthy(); // footer hint

    fireEvent.change(screen.getByLabelText(/^Artist/), { target: { value: "KIRA" } });
    await screen.findAllByText("VX-L01_KIRA");
    await waitFor(() => expect((create as HTMLButtonElement).disabled).toBe(false));
  });

  it("No client hides the client field and lets you create with the artist empty", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    const create = (await screen.findByRole("button", { name: /^Create/ })) as HTMLButtonElement;
    await screen.findAllByText("VX-L01_Artist");
    expect(create.disabled).toBe(true);
    expect(screen.getByText("D:/NestDev")).toBeTruthy();

    fireEvent.click(screen.getByLabelText("No client (personal or passion project)"));
    // It goes to the Personal folder chosen in Settings (shown from the Plan, once it's back).
    expect(await screen.findByText("D:/Personal")).toBeTruthy();
    expect(screen.queryByPlaceholderText("Name")).toBeNull(); // the client name box is gone
    expect(screen.getByText(/Every question is optional/)).toBeTruthy();
    await screen.findAllByText("OWN-L01");
    await waitFor(() => expect(create.disabled).toBe(false));

    // In the Personal space it's always on and can't be unticked.
    fireEvent.click(screen.getByRole("button", { name: "Change…" }));
    expect(screen.getByText("Your Personal folder")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Personal" }));
    fireEvent.click(screen.getByLabelText("No client (personal or passion project)"));
    const box = screen.getByLabelText("No client (personal or passion project)") as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(box.disabled).toBe(true);
    expect(screen.getByText(/Personal never has a client/)).toBeTruthy();
  });

  it("suggests known clients and fills the code", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    const name = await screen.findByPlaceholderText("Name");
    fireEvent.focus(name);
    fireEvent.change(name, { target: { value: "ver" } });
    fireEvent.mouseDown(await screen.findByRole("option", { name: /Vertex Studio/ }));
    expect((screen.getByPlaceholderText("Name") as HTMLInputElement).value).toBe("Vertex Studio");
    expect((screen.getByLabelText("Client code") as HTMLInputElement).value).toBe("VX");
  });

  it("says nothing about client files until there's a client with some", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    await screen.findAllByText("VX-L01_Artist");
    expect(shown()).not.toContain("'s files");
    expect(shown()).not.toMatch(/kit/i);
  });

  it("shows the client's files under the Client field and sends their fingerprint with Create", async () => {
    const restore = withKit(KIT);
    const create = vi.mocked(commands.createProject);
    create.mockResolvedValue({ key: "k", jobCode: "VX-L01", title: "KIRA", root: "D:/NestDev/x", bumped: false });
    const onCreated = vi.fn();
    render(<NewProjectSheet onClose={() => {}} onCreated={onCreated} />);
    expect(await screen.findByText(/^Vertex Studio's files: 38 files · 1\.2 GB ·/)).toBeTruthy();
    expect(screen.getByText(/Online-only files will be downloaded while copying/)).toBeTruthy();
    expect(shown()).not.toMatch(/kit/i);
    // Their first files show in the preview, inside their folder; the rest are counted.
    expect(screen.getByText("logo.png")).toBeTruthy();
    expect(screen.getByText("…and 37 more files")).toBeTruthy();
    // Change = choose another folder for this client.
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    await waitFor(() => expect(commands.chooseClientKit).toHaveBeenCalledWith("VX", "Vertex Studio"));

    fireEvent.change(screen.getByLabelText(/^Artist/), { target: { value: "KIRA" } });
    const button = (await screen.findByRole("button", { name: /^Create/ })) as HTMLButtonElement;
    await waitFor(() => expect(button.disabled).toBe(false));
    fireEvent.click(button);
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
    expect(create).toHaveBeenCalledWith(expect.objectContaining({ kitFingerprint: "38-1200000000-abc" }));
    restore();
  });

  it("offers Add… when the client has no files, and hides the line for No client", async () => {
    const restore = withKit(NONE);
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByText(/^Vertex Studio's files: none ·/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add…" }));
    await waitFor(() => expect(commands.chooseClientKit).toHaveBeenCalledWith("VX", "Vertex Studio"));

    fireEvent.click(screen.getByLabelText("No client (personal or passion project)"));
    expect(screen.queryByText(/Vertex Studio's files/)).toBeNull();
    restore();
  });

  it("asks where to put the client's files when the template has no place, and saves the choice", async () => {
    const restore = withKit({ ...NONE, state: "noPlace", source: "D:/Kits/VX" });
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    const select = (await screen.findByLabelText(/Vertex Studio's files · Put them in/)) as HTMLSelectElement;
    // Folders that are always made, top-level and nested; not "only if" or "one per" ones.
    const options = Array.from(select.options).map((o) => o.value);
    expect(options).toEqual(["", "01_REF", "02_BRAND", "02_BRAND/LOGOS"]);
    const contexts = vi.mocked(commands.newProjectContext).mock.calls.length;
    const plans = vi.mocked(commands.planProject).mock.calls.length;

    fireEvent.change(select, { target: { value: "02_BRAND" } });
    await waitFor(() => expect(commands.setTemplateKitFolder).toHaveBeenCalledWith("grading-mv", "02_BRAND"));
    // Then the template is read again and the preview planned again.
    await waitFor(() => expect(vi.mocked(commands.newProjectContext).mock.calls.length).toBe(contexts + 1));
    await waitFor(() => expect(vi.mocked(commands.planProject).mock.calls.length).toBeGreaterThan(plans));
    restore();
  });

  it("says when the client's files couldn't be read", async () => {
    const restore = withKit({ ...NONE, state: "unreadable", source: "E:/Kits/VX" });
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByText(/^Couldn't read Vertex Studio's files ·/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Choose a folder…" }));
    await waitFor(() => expect(commands.chooseClientKit).toHaveBeenCalledWith("VX", "Vertex Studio"));
    restore();
  });
});

/** Opens the sheet with one template made of these fields and tree rules. */
async function openWith(fields: Field[], tree: TreeEntry[]) {
  const base = await vi.mocked(commands.newProjectContext)();
  const tpl = { ...base.templates[0], fields: [base.templates[0].fields[1], ...fields], tree };
  vi.mocked(commands.newProjectContext).mockResolvedValueOnce({ ...base, templates: [tpl] });
  render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
  await screen.findByLabelText(/^Artist/);
}

const bool = (key: string, label: string, def = false): Field => ({ key, label, type: "bool", default: def });

/** Opens the sheet with the first template carrying a Resolve project; plans include it while
 *  the box is on (as Rust does). */
async function openWithResolve(on: boolean) {
  const base = await vi.mocked(commands.newProjectContext)();
  const tpl = {
    ...base.templates[0],
    resolve: { drp: "files/Grade.drp", name: "{artist}", folder: "{client}/2026", on },
  };
  vi.mocked(commands.newProjectContext).mockResolvedValueOnce({ ...base, templates: [tpl] });
  const plan = vi.mocked(commands.planProject);
  const basePlan = plan.getMockImplementation()!;
  plan.mockImplementation(async (req) => {
    const p = await basePlan(req);
    const wanted = req.resolve ?? on;
    return wanted
      ? {
          ...p,
          resolve: {
            drp: "files/Grade.drp",
            name: p.title || "Artist",
            folder: "Vertex Studio/2026",
            library: null,
            libraryLabel: "the library open in Resolve",
          },
        }
      : p;
  });
  const onCreated = vi.fn();
  render(<NewProjectSheet onClose={() => {}} onCreated={onCreated} />);
  await screen.findByLabelText(/^Artist/);
  return { onCreated, restore: () => plan.mockImplementation(basePlan) };
}

describe("Resolve project", () => {
  it("is a box in Also make:, ticked like the template, with the preview line; Create sends it", async () => {
    const { onCreated, restore } = await openWithResolve(true);
    expect(screen.getByText("Also make:")).toBeTruthy();
    const box = screen.getByLabelText("Resolve project") as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(await screen.findByText("Adds “Artist” to DaVinci Resolve, in Vertex Studio › 2026")).toBeTruthy();

    fireEvent.change(screen.getByLabelText(/^Artist/), { target: { value: "KIRA" } });
    expect(await screen.findByText("Adds “KIRA” to DaVinci Resolve, in Vertex Studio › 2026")).toBeTruthy();
    const create = vi.mocked(commands.createProject);
    create.mockResolvedValue({ key: "k", jobCode: "VX-L01", title: "KIRA", root: "D:/NestDev/x", bumped: false });
    const button = screen.getByRole("button", { name: /^Create/ }) as HTMLButtonElement;
    await waitFor(() => expect(button.disabled).toBe(false));
    fireEvent.click(button);
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
    // null = the template's default; the success line learns the project's name.
    expect(create).toHaveBeenCalledWith(expect.objectContaining({ resolve: null }));
    expect(onCreated.mock.lastCall![1]).toBe("KIRA");
    restore();
  });

  it("unticking sends resolve: false and the preview line goes", async () => {
    const { restore } = await openWithResolve(true);
    await screen.findByText(/^Adds “.*” to DaVinci Resolve/);
    fireEvent.click(screen.getByLabelText("Resolve project"));
    await waitFor(() => expect(vi.mocked(commands.planProject).mock.lastCall![0].resolve).toBe(false));
    await waitFor(() => expect(screen.queryByText(/^Adds “.*” to DaVinci Resolve/)).toBeNull());
    restore();
  });

  it("starts unticked when the template says so", async () => {
    const { restore } = await openWithResolve(false);
    expect((screen.getByLabelText("Resolve project") as HTMLInputElement).checked).toBe(false);
    expect(screen.queryByText(/^Adds “.*” to DaVinci Resolve/)).toBeNull();
    fireEvent.click(screen.getByLabelText("Resolve project"));
    expect(await screen.findByText(/^Adds “Artist” to DaVinci Resolve, in/)).toBeTruthy();
    restore();
  });

  it("shows nothing for a template without one", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    await screen.findAllByText("VX-L01_Artist");
    expect(screen.queryByLabelText("Resolve project")).toBeNull();
    expect(shown()).not.toContain("Resolve");
  });
});

describe("Also make:", () => {
  it("shows a yes/no that makes a folder, labelled after the folder, unticked, with the path as tooltip", async () => {
    await openWith([bool("print", "Includes print")], ["01_BRIEF", { name: "05_EXPORTS/PRINT", when: "print" }]);
    expect(screen.getByText("Also make:")).toBeTruthy();
    const box = screen.getByLabelText("PRINT folder") as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(box.closest("label")?.getAttribute("title")).toBe("05_EXPORTS/PRINT");
    // Its own question is gone from the normal list.
    expect(screen.queryByLabelText("Includes print")).toBeNull();
    // Ticking sends it in the plan request.
    fireEvent.click(box);
    await waitFor(() => {
      const last = vi.mocked(commands.planProject).mock.lastCall![0];
      expect(last.values.print).toBe(true);
    });
  });

  it("keeps the template's default (ticked stays ticked)", async () => {
    await openWith([bool("print", "Includes print", true)], [{ name: "PRINT", when: "print" }]);
    expect((screen.getByLabelText("PRINT folder") as HTMLInputElement).checked).toBe(true);
  });

  it("names several folders on one key", async () => {
    await openWith([bool("print", "Includes print")], [
      { name: "05_EXPORTS/PRINT", when: "print" },
      { name: "06/PROOFS", when: "print" },
    ]);
    const box = screen.getByLabelText("PRINT and PROOFS folders");
    expect(box.closest("label")?.getAttribute("title")).toBe("05_EXPORTS/PRINT\n06/PROOFS");
  });

  it("uses the question's label when the folder name has blanks", async () => {
    await openWith([bool("print", "Includes print")], [{ name: "05/{artist}_PRINT", when: "print" }]);
    expect(screen.getByLabelText("Includes print")).toBeTruthy();
    expect(screen.getByText("Also make:")).toBeTruthy();
  });

  it("leaves a yes/no tied to no folder as a normal question, with no group", async () => {
    await openWith([bool("rush", "Rush job")], ["01_BRIEF"]);
    expect(screen.getByLabelText("Rush job")).toBeTruthy();
    expect(screen.queryByText("Also make:")).toBeNull();
  });

  it("leaves a !key rule as a normal question", async () => {
    await openWith([bool("print", "Skip print")], [{ name: "05/NO_PRINT", when: "!print" }]);
    expect(screen.getByLabelText("Skip print")).toBeTruthy();
    expect(screen.queryByText("Also make:")).toBeNull();
  });
});

describe("helpers", () => {
  it("lists the folders client files can go in", () => {
    expect(clientFilePlaces(undefined)).toEqual([]);
    expect(
      clientFilePlaces(["A", "A/B", { name: "C/D", kit: true }, { name: "E", when: "x" }, { name: "F/{item}", each: "y" }]),
    ).toEqual(["A", "A/B", "C", "C/D"]);
  });

  it("shortens long folder paths and keeps short ones", () => {
    expect(shortPath("D:\\Jobs")).toBe("D:\\Jobs");
    expect(shortPath("D:\\Work\\Clients\\Everything\\2026\\Jobs")).toBe("D:\\…\\2026\\Jobs");
    expect(shortPath("/Users/me/Library/CloudStorage/Drive/Jobs")).toBe("/…/Drive/Jobs");
  });
});
