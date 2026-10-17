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

function view(showInMenuBar: boolean): SettingsView {
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
