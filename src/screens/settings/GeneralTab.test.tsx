// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

const setShowInMenuBar = vi.fn(async (on: boolean) => ({ on }));
vi.mock("../../lib/commands", () => ({
  addJobsRoot: vi.fn(),
  clearArchiveFolder: vi.fn(),
  forgetPastArchiveFolder: vi.fn(),
  moveJobsRoot: vi.fn(),
  removeJobsRoot: vi.fn(),
  setArchiveAfterDays: vi.fn(),
  setArchiveFolder: vi.fn(),
  setDefaultSpace: vi.fn(),
  setLaunchAtLogin: vi.fn(),
  setPersonalRoot: vi.fn(),
  setShowInMenuBar: (on: boolean) => setShowInMenuBar(on),
  reusePastArchiveFolder: vi.fn(),
}));

import GeneralTab from "./GeneralTab";
import type { SettingsView } from "../../lib/types";

function view(showInMenuBar: boolean, loginItem: SettingsView["loginItem"] = "off"): SettingsView {
  return {
    settings: {
      jobsRoots: [],
      spaces: ["Work", "Personal"],
      defaultSpace: "Work",
      noClientSpaces: ["Personal"],
      launchAtLogin: false,
      showInMenuBar,
      closeNoteShown: false,
      archiveFolder: null,
      archiveAfterDays: null,
      personalRoot: null,
      pastArchiveFolders: [],
    },
    settingsError: null,
    firstRun: false,
    templatesFolder: "/t",
    devBuild: false,
    suggestedMarker: "M",
    loginItem,
  } as unknown as SettingsView;
}

const run = <T,>(p: Promise<T>) => p;

afterEach(() => {
  cleanup();
  delete document.documentElement.dataset.platform;
  setShowInMenuBar.mockClear();
});

describe("GeneralTab: menu bar icon", () => {
  it("on a Mac says menu bar, and turning it off is sent to Rust", () => {
    document.documentElement.dataset.platform = "mac";
    render(<GeneralTab view={view(true)} run={run} />);
    const box = screen.getByLabelText("Show Nest in the menu bar") as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(screen.getByText(/keeps Nest running in the menu bar/)).toBeTruthy();
    fireEvent.click(box);
    expect(setShowInMenuBar).toHaveBeenCalledWith(false);
  });

  it("on Windows says notification area; off says closing quits", () => {
    document.documentElement.dataset.platform = "win";
    render(<GeneralTab view={view(false)} run={run} />);
    const box = screen.getByLabelText("Keep Nest in the notification area") as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(screen.getByText("Closing the window quits Nest.")).toBeTruthy();
    expect((screen.getByLabelText("Open Nest when you log in to this computer") as HTMLInputElement).checked).toBe(false);
  });
});

describe("GeneralTab: open at login", () => {
  const box = () => screen.getByLabelText("Open Nest when you log in to this computer") as HTMLInputElement;

  it("shows what the system has, not the last click", () => {
    render(<GeneralTab view={view(true, "on")} run={run} />);
    expect(box().checked).toBe(true);
    expect(box().disabled).toBe(false);
  });

  it("waiting for approval: ticked, with where to allow it", () => {
    document.documentElement.dataset.platform = "mac";
    render(<GeneralTab view={view(true, "needsApproval")} run={run} />);
    expect(box().checked).toBe(true);
    expect(screen.getByText("Allow Nest in System Settings → General → Login Items.")).toBeTruthy();
  });

  it("running from the disk image: off, can't be turned on, says why", () => {
    document.documentElement.dataset.platform = "mac";
    render(<GeneralTab view={view(true, "moveFirst")} run={run} />);
    expect(box().checked).toBe(false);
    expect(box().disabled).toBe(true);
    expect(screen.getByText("Move Nest to your Applications folder first.")).toBeTruthy();
  });

  it("dev builds can't change it", () => {
    render(<GeneralTab view={view(false, "devBuild")} run={run} />);
    expect(box().disabled).toBe(true);
    expect(screen.getByText("Only available in the installed app.")).toBeTruthy();
  });
});
