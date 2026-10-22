// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { DevicesView, SettingsView } from "../../lib/types";

const setCodeSettings = vi.fn<(pattern: string, marker: string, own: string) => Promise<{ ok: boolean }>>(async () => ({ ok: true }));
const devicesSetName = vi.fn<(name: string) => Promise<DevicesView>>();
const devicesView = vi.fn();
vi.mock("../../lib/commands", () => ({
  setCodeSettings: (p: string, m: string, o: string) => setCodeSettings(p, m, o),
  devicesSetName: (n: string) => devicesSetName(n),
  devicesView: () => devicesView(),
  devicesDetect: vi.fn(async () => []),
  devicesEnable: vi.fn(),
  devicesDisable: vi.fn(),
  devicesForget: vi.fn(),
  devicesClaim: vi.fn(),
  devicesPickFolder: vi.fn(),
  devicesSetSyncTemplates: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import DevicesTab from "./DevicesTab";

const pc = { deviceId: "1", name: "PC", os: "windows", letter: "W", guessed: false };

function dev(extra: Partial<DevicesView> = {}): DevicesView {
  return {
    enabled: false,
    deviceId: null,
    name: "MacBook",
    folder: null,
    savedAt: null,
    problem: null,
    thisProjectCount: 0,
    others: [],
    forgotten: [],
    syncTemplates: false,
    templateProblem: null,
    letters: [],
    thisLetter: "MB",
    letterClash: [],
    suggestedLetter: "P",
    copiedNote: false,
    letterClashKept: "",
    systemName: "MacBook",
    ...extra,
  } as unknown as DevicesView;
}

function view(localMarker = "MB"): SettingsView {
  return {
    settings: { codePattern: "{clientCode}-{marker}{seq:02}", localMarker, ownCode: "OWN" },
  } as unknown as SettingsView;
}

const run = <T,>(p: Promise<T>) => p;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Settings → Computers: this computer", () => {
  it("name and letter are editable with sharing off, and the box comes first", async () => {
    devicesView.mockResolvedValue(dev());
    devicesSetName.mockResolvedValue(dev({ name: "Studio Mac" }));
    render(<DevicesTab view={view()} run={run} />);
    const name = (await screen.findByLabelText("Name")) as HTMLInputElement;
    expect(name.value).toBe("MacBook");
    expect(screen.getByRole("heading", { name: "This computer" })).toBeTruthy();
    expect(screen.getByText(/Every computer has its own letter, so two never make the same code/)).toBeTruthy();
    expect(screen.getByText("ACME-MB04")).toBeTruthy();
    fireEvent.change(name, { target: { value: "  Studio Mac " } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(devicesSetName).toHaveBeenCalledWith("Studio Mac"));
    expect(setCodeSettings).not.toHaveBeenCalled();
  });

  it("Save stays off for an empty name", async () => {
    devicesView.mockResolvedValue(dev());
    render(<DevicesTab view={view()} run={run} />);
    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "  " } });
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("saves a changed letter with the saved style and personal code", async () => {
    devicesView.mockResolvedValue(dev());
    render(<DevicesTab view={view()} run={run} />);
    fireEvent.change(await screen.findByLabelText("Letter"), { target: { value: "q" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(setCodeSettings).toHaveBeenCalledWith("{clientCode}-{marker}{seq:02}", "Q", "OWN"));
    expect(devicesSetName).not.toHaveBeenCalled();
  });

  it("takes 1 or 2 letters and drops digits as you type", async () => {
    devicesView.mockResolvedValue(dev());
    render(<DevicesTab view={view()} run={run} />);
    const box = (await screen.findByLabelText("Letter")) as HTMLInputElement;
    expect(box.maxLength).toBe(2);
    fireEvent.change(box, { target: { value: "w1l" } });
    expect(box.value).toBe("WL");
  });

  it("refuses a letter starting with J and keeps Save off", async () => {
    devicesView.mockResolvedValue(dev());
    render(<DevicesTab view={view()} run={run} />);
    fireEvent.change(await screen.findByLabelText("Letter"), { target: { value: "JB" } });
    const problem = screen.getByText(/can't start with J/);
    expect(problem.getAttribute("role")).toBe("status");
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("the letter warning is one stable status element whose words change as you type", async () => {
    devicesView.mockResolvedValue(dev({ letters: [pc], letterClash: [pc], thisLetter: "W" }));
    render(<DevicesTab view={view("W")} run={run} />);
    const first = await screen.findByText(/W is used by your PC/);
    expect(first.getAttribute("role")).toBe("status");
    expect(first.querySelector("button")).toBeNull();
    fireEvent.change(screen.getByLabelText("Letter"), { target: { value: "Q" } });
    expect(first.isConnected).toBe(true);
    expect(first.textContent).toBe("");
    fireEvent.change(screen.getByLabelText("Letter"), { target: { value: "W" } });
    expect(first.textContent).toMatch(/W is used by your PC/);
    expect(screen.queryAllByRole("alert")).toHaveLength(0);
  });

  it("a taken letter offers a free one as one button, and saves only the letter", async () => {
    devicesView.mockResolvedValue(dev({ letters: [pc], letterClash: [pc], thisLetter: "W" }));
    render(<DevicesTab view={view("W")} run={run} />);
    expect(await screen.findByText(/W is used by your PC\. Use P instead\?/)).toBeTruthy();
    expect(setCodeSettings).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Use P" }));
    await waitFor(() => expect(setCodeSettings).toHaveBeenCalledWith("{clientCode}-{marker}{seq:02}", "P", "OWN"));
    expect((screen.getByLabelText("Letter") as HTMLInputElement).value).toBe("P");
  });

  it("Keep W hides the warning without changing anything", async () => {
    devicesView.mockResolvedValue(dev({ letters: [pc], letterClash: [pc], thisLetter: "W" }));
    render(<DevicesTab view={view("W")} run={run} />);
    fireEvent.click(await screen.findByRole("button", { name: "Keep W" }));
    expect(screen.queryByText(/W is used by/)).toBeNull();
    expect(setCodeSettings).not.toHaveBeenCalled();
  });

  it("warns when you type a letter another computer uses, not for a free one or WL", async () => {
    devicesView.mockResolvedValue(dev({ letters: [pc] }));
    render(<DevicesTab view={view()} run={run} />);
    const box = await screen.findByLabelText("Letter");
    expect(screen.queryByText(/is used by/)).toBeNull();
    fireEvent.change(box, { target: { value: "w" } });
    expect(await screen.findByText(/W is used by your PC/)).toBeTruthy();
    fireEvent.change(box, { target: { value: "WL" } });
    expect(screen.queryByText(/is used by/)).toBeNull();
  });

  it("says the numbers carry on when the letter changes", async () => {
    devicesView.mockResolvedValue(dev());
    render(<DevicesTab view={view("W")} run={run} />);
    fireEvent.change(await screen.findByLabelText("Letter"), { target: { value: "WL" } });
    expect(screen.getByText(/W05 is followed by WL06/)).toBeTruthy();
  });

  it("opened from the same-letter note, the letter field has the cursor", async () => {
    devicesView.mockResolvedValue(dev());
    render(<DevicesTab view={view()} run={run} focusLetter />);
    const box = await screen.findByLabelText("Letter");
    await waitFor(() => expect(document.activeElement).toBe(box));
  });

  it("sharing stays below, with its own heading", async () => {
    devicesView.mockResolvedValue(dev());
    render(<DevicesTab view={view()} run={run} />);
    expect(await screen.findByRole("heading", { name: "Share with your other computers" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Turn on Devices" })).toBeTruthy();
  });
});
