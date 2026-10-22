// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { DevicesView, SettingsView } from "../../lib/types";

const setCodeSettings = vi.fn<(pattern: string, marker: string, own: string) => Promise<{ ok: boolean }>>(async () => ({ ok: true }));
const devicesView = vi.fn();
vi.mock("../../lib/commands", () => ({
  setCodeSettings: (p: string, m: string, o: string) => setCodeSettings(p, m, o),
  devicesView: () => devicesView(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import CodesTab from "./CodesTab";

const pc = { deviceId: "1", name: "PC", os: "windows", letter: "W", guessed: false };

function dev(extra: Partial<DevicesView> = {}): DevicesView {
  return {
    enabled: true,
    deviceId: "m",
    name: "MacBook",
    folder: "/Drive",
    savedAt: null,
    problem: null,
    thisProjectCount: 0,
    others: [],
    forgotten: [],
    syncTemplates: false,
    templateProblem: null,
    letters: [pc],
    thisLetter: "W",
    letterClash: [pc],
    suggestedLetter: "P",
    copiedNote: false,
    letterClashKept: "",
    systemName: "MacBook",
    ...extra,
  };
}

function view(codePattern = "{clientCode}-{marker}{seq:02}", localMarker = "W"): SettingsView {
  return {
    settings: { codePattern, localMarker, ownCode: "OWN", noClientSpaces: ["Personal"] },
  } as unknown as SettingsView;
}

const run = <T,>(p: Promise<T>) => p;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Settings → Job codes: the computer's letter", () => {
  it("shows the letter and has no letter field", () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    render(<CodesTab view={view("{clientCode}-{marker}{seq:02}", "MB")} run={run} goTo={vi.fn()} />);
    expect(screen.getByText(/This computer's letter:/).textContent).toContain("MB");
    expect(screen.queryByLabelText(/Letter/)).toBeNull();
  });

  it("'Change in Computers' switches tab", () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    const goTo = vi.fn();
    render(<CodesTab view={view()} run={run} goTo={goTo} />);
    fireEvent.click(screen.getByRole("button", { name: "Change in Computers" }));
    expect(goTo).toHaveBeenCalledWith("computers");
  });

  it("Save keeps the saved letter and saves only the style and personal code", async () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    render(<CodesTab view={view("{clientCode}-{marker}{seq:02}", "MB")} run={run} goTo={vi.fn()} />);
    fireEvent.click(screen.getAllByRole("radio")[1]);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(setCodeSettings).toHaveBeenCalledWith("{clientCode}-{marker}{seq:03}", "MB", "OWN"));
  });
});

describe("Settings → Job codes: styles", () => {
  it("a style without the letter warns only when sharing is on, with a one-button fix", async () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    render(<CodesTab view={view("{clientCode}-{seq:02}")} run={run} />);
    expect(await screen.findByText(/leaves out the computer's letter/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Use ACME-W04" }));
    await waitFor(() => expect(setCodeSettings).toHaveBeenCalledWith("{clientCode}-{marker}{seq:02}", "W", "OWN"));
  });

  it("is quiet when sharing is off", async () => {
    devicesView.mockResolvedValue(dev({ enabled: false, letters: [], letterClash: [] }));
    render(<CodesTab view={view("{clientCode}-{seq:02}")} run={run} />);
    await waitFor(() => expect(devicesView).toHaveBeenCalled());
    expect(screen.queryByText(/leaves out the computer's letter/)).toBeNull();
  });

  it("two letters with no dash warn and offer a dash", async () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    render(<CodesTab view={view("{clientCode}{marker}{seq:02}", "WL")} run={run} />);
    expect(await screen.findByText(/can be read two ways/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add a dash" }));
    await waitFor(() => expect(setCodeSettings).toHaveBeenCalledWith("{clientCode}-{marker}{seq:02}", "WL", "OWN"));
  });
});
