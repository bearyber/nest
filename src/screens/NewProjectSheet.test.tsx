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
  chooseClientKit: vi.fn(async () => null),
  cancelCreate: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import * as commands from "../lib/commands";
import type { PlanKit } from "../lib/types";
import NewProjectSheet from "./NewProjectSheet";

/** Plans like the stand-in above, plus this client kit. Returns a way to go back. */
function withKit(kit: PlanKit) {
  const plan = vi.mocked(commands.planProject);
  const base = plan.getMockImplementation()!;
  plan.mockImplementation(async (req) => ({ ...(await base(req)), kit }));
  return () => plan.mockImplementation(base);
}

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

afterEach(cleanup);

describe("NewProjectSheet", () => {
  it("renders fields from the template", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    // lastTemplate names a removed template, so the first one is picked and its fields show.
    expect(await screen.findByLabelText(/^Artist/)).toBeTruthy();
    expect(screen.getByPlaceholderText("Name")).toBeTruthy();
    // The sheet always calls the paying party "Client", whatever the template says.
    expect(screen.getByText("Client")).toBeTruthy();
    expect(screen.queryByText("Billed to")).toBeNull();
    expect(screen.getByLabelText("Client code")).toBeTruthy();
    const lookdev = screen.getByLabelText("Include look dev") as HTMLInputElement;
    expect(lookdev.type).toBe("checkbox");
    expect(lookdev.checked).toBe(false);
    // Ratios are toggle chips; the template default (16x9) starts on.
    const on = screen.getByRole("button", { name: "16x9" });
    const off = screen.getByRole("button", { name: "9x16" });
    expect(on.getAttribute("aria-pressed")).toBe("true");
    expect(off.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(off);
    expect(off.getAttribute("aria-pressed")).toBe("true");
    // Location has a real Change… button; space is a segmented control.
    expect(screen.getByRole("button", { name: "Change…" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Work" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("keeps Create disabled until required fields are valid", async () => {
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    const create = await screen.findByRole("button", { name: /^Create/ });
    // Shown twice: the Folder row and the preview tree's root.
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
    expect(screen.queryByText("Your Personal folder")).toBeNull();

    fireEvent.click(screen.getByLabelText("No client (personal or passion project)"));
    // It goes to the Personal folder chosen in Settings (shown from the Plan, once it's back).
    expect(await screen.findByText("D:/Personal")).toBeTruthy();
    expect(screen.getByText("Your Personal folder")).toBeTruthy();
    expect(screen.queryByPlaceholderText("Name")).toBeNull(); // the client name box is gone
    expect(screen.getByText(/Every question is optional/)).toBeTruthy();
    await screen.findAllByText("OWN-L01");
    await waitFor(() => expect(create.disabled).toBe(false));

    // In the Personal space it's always on and can't be unticked.
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

  it("shows the client kit and sends its fingerprint with Create", async () => {
    const restore = withKit(KIT);
    const create = vi.mocked(commands.createProject);
    create.mockResolvedValue({ key: "k", jobCode: "VX-L01", title: "KIRA", root: "D:/NestDev/x", bumped: false });
    const onCreated = vi.fn();
    render(<NewProjectSheet onClose={() => {}} onCreated={onCreated} />);
    expect(await screen.findByText(/Client kit: Vertex Studio · 38 files · 1\.2 GB/)).toBeTruthy();
    expect(screen.getByText(/Online-only files will be downloaded while copying/)).toBeTruthy();
    // The kit's first files show in the preview, inside its folder; the rest are counted.
    expect(screen.getByText("logo.png")).toBeTruthy();
    expect(screen.getByText("…and 37 more files from the kit")).toBeTruthy();

    fireEvent.change(screen.getByLabelText(/^Artist/), { target: { value: "KIRA" } });
    const button = (await screen.findByRole("button", { name: /^Create/ })) as HTMLButtonElement;
    await waitFor(() => expect(button.disabled).toBe(false));
    fireEvent.click(button);
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
    expect(create).toHaveBeenCalledWith(expect.objectContaining({ kitFingerprint: "38-1200000000-abc" }));
    restore();
  });

  it("offers to choose a kit folder when the client has none, and explains a template without a place", async () => {
    const restore = withKit({ ...KIT, state: "missing", source: undefined, fingerprint: undefined, count: 0, bytes: 0, sample: [] });
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByText(/No kit for Vertex Studio/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Choose a folder…" }));
    await waitFor(() => expect(commands.chooseClientKit).toHaveBeenCalledWith("VX", "Vertex Studio"));
    restore();
    cleanup();

    const restore2 = withKit({ ...KIT, state: "noPlace", fingerprint: undefined, count: 0, bytes: 0, sample: [] });
    render(<NewProjectSheet onClose={() => {}} onCreated={() => {}} />);
    expect(await screen.findByText(/This template has no place for a client kit/)).toBeTruthy();
    restore2();
  });
});
