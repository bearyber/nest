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
  it("takes 1 or 2 letters and drops digits as you type", () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    render(<CodesTab view={view()} run={run} />);
    const box = screen.getByLabelText("Letter for this computer") as HTMLInputElement;
    expect(box.maxLength).toBe(2);
    fireEvent.change(box, { target: { value: "w1l" } });
    expect(box.value).toBe("WL");
  });

  it("refuses a letter starting with J and keeps Save off", () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    render(<CodesTab view={view()} run={run} />);
    fireEvent.change(screen.getByLabelText("Letter for this computer"), { target: { value: "JB" } });
    expect(screen.getByText(/can't start with J/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("the letter warning is one stable status element whose words change as you type", async () => {
    devicesView.mockResolvedValue(dev());
    render(<CodesTab view={view()} run={run} />);
    const first = await screen.findByText(/W is used by your PC/);
    expect(first.getAttribute("role")).toBe("status");
    expect(first.querySelector("button")).toBeNull();
    // A free letter: the words go, the element stays.
    fireEvent.change(screen.getByLabelText("Letter for this computer"), { target: { value: "Q" } });
    expect(first.isConnected).toBe(true);
    expect(first.textContent).toBe("");
    // Taken again: the same element has words again.
    fireEvent.change(screen.getByLabelText("Letter for this computer"), { target: { value: "W" } });
    expect(first.textContent).toMatch(/W is used by your PC/);
    expect(screen.queryAllByRole("alert")).toHaveLength(0);
  });

  it("a taken letter says who uses it and offers a free one as one button", async () => {
    devicesView.mockResolvedValue(dev());
    render(<CodesTab view={view()} run={run} />);
    expect(await screen.findByText(/W is used by your PC\. Use P instead\?/)).toBeTruthy();
    // Nothing was changed by itself.
    expect(setCodeSettings).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Use P" }));
    await waitFor(() => expect(setCodeSettings).toHaveBeenCalledWith("{clientCode}-{marker}{seq:02}", "P", "OWN"));
    expect((screen.getByLabelText("Letter for this computer") as HTMLInputElement).value).toBe("P");
  });

  it("Keep W hides the warning without changing anything", async () => {
    devicesView.mockResolvedValue(dev());
    render(<CodesTab view={view()} run={run} />);
    fireEvent.click(await screen.findByRole("button", { name: "Keep W" }));
    expect(screen.queryByText(/W is used by/)).toBeNull();
    expect(setCodeSettings).not.toHaveBeenCalled();
  });

  it("warns when you type a letter another computer uses, and not for a free one or WL", async () => {
    devicesView.mockResolvedValue(dev({ thisLetter: "M", letterClash: [] }));
    render(<CodesTab view={view("{clientCode}-{marker}{seq:02}", "M")} run={run} />);
    await waitFor(() => expect(devicesView).toHaveBeenCalled());
    const box = screen.getByLabelText("Letter for this computer");
    expect(screen.queryByText(/is used by/)).toBeNull();
    fireEvent.change(box, { target: { value: "w" } });
    expect(await screen.findByText(/W is used by your PC/)).toBeTruthy();
    fireEvent.change(box, { target: { value: "WL" } });
    expect(screen.queryByText(/is used by/)).toBeNull();
  });

  it("an older computer's letter is worded as a guess", async () => {
    devicesView.mockResolvedValue(dev({ letters: [{ ...pc, guessed: true }] }));
    render(<CodesTab view={view()} run={run} />);
    expect(await screen.findByText(/W seems to be used by your PC/)).toBeTruthy();
  });

  it("says the numbers carry on when the letter changes", () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    render(<CodesTab view={view()} run={run} />);
    fireEvent.change(screen.getByLabelText("Letter for this computer"), { target: { value: "WL" } });
    expect(screen.getByText(/W05 is followed by WL06/)).toBeTruthy();
  });
});

describe("Settings → Job codes: styles", () => {
  it("a style without the letter warns only when Devices is on, with a one-button fix", async () => {
    devicesView.mockResolvedValue(dev({ letters: [], letterClash: [] }));
    render(<CodesTab view={view("{clientCode}-{seq:02}")} run={run} />);
    expect(await screen.findByText(/leaves out the computer's letter/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Use ACME-W04" }));
    await waitFor(() => expect(setCodeSettings).toHaveBeenCalledWith("{clientCode}-{marker}{seq:02}", "W", "OWN"));
  });

  it("is quiet when Devices is off", async () => {
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
