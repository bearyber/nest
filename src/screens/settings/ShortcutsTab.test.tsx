// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

const setShortcuts = vi.fn(async (s: unknown) => ({ saved: s }));
const shortcutRecording = vi.fn(async (on: boolean) => on);
vi.mock("../../lib/commands", () => ({
  setShortcuts: (s: unknown) => setShortcuts(s),
  shortcutRecording: (on: boolean) => shortcutRecording(on),
  getSettings: vi.fn(),
  openSettings: vi.fn(),
}));

import ShortcutsTab from "./ShortcutsTab";
import type { SettingsView } from "../../lib/types";

const view = (shortcuts: Record<string, unknown> = {}) => ({ settings: { shortcuts } }) as unknown as SettingsView;
const run = <T,>(p: Promise<T>) => p;

afterEach(() => {
  cleanup();
  delete document.documentElement.dataset.platform;
  setShortcuts.mockClear();
  shortcutRecording.mockClear();
});

const press = (init: KeyboardEventInit) => {
  const e = new KeyboardEvent("keydown", { cancelable: true, bubbles: true, ...init });
  act(() => {
    window.dispatchEvent(e);
  });
  return e;
};
const start = async (name: RegExp) => {
  fireEvent.click(screen.getByRole("button", { name }));
  await screen.findByText("Type the new keys…");
};

describe("Shortcuts tab", () => {
  it("lists the shortcuts with their keys for this OS", () => {
    document.documentElement.dataset.platform = "mac";
    render(<ShortcutsTab view={view()} run={run} />);
    expect(screen.getByRole("button", { name: /^New Project: ⌘N/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /^Quit Nest: ⌘Q/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /^Show Keyboard Shortcuts: no keys/ })).toBeTruthy();
    expect(screen.queryByText("Reset All")).toBeNull();
    cleanup();
    document.documentElement.dataset.platform = "win";
    render(<ShortcutsTab view={view()} run={run} />);
    expect(screen.getByRole("button", { name: /^New Project: Ctrl\+N/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /^Show Keyboard Shortcuts: F1/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Quit Nest/ })).toBeNull();
  });

  it("records new keys, swallowing the key press, and saves them", async () => {
    document.documentElement.dataset.platform = "mac";
    render(<ShortcutsTab view={view()} run={run} />);
    await start(/^Find: /);
    expect(shortcutRecording).toHaveBeenCalledWith(true);
    const e = press({ key: "g", code: "KeyG", metaKey: true });
    expect(e.defaultPrevented).toBe(true);
    await waitFor(() => expect(setShortcuts).toHaveBeenCalledWith({ search: { key: "g", mod: true } }));
    await waitFor(() => expect(shortcutRecording).toHaveBeenCalledWith(false));
  });

  it("Escape cancels without saving and without reaching other handlers", async () => {
    document.documentElement.dataset.platform = "win";
    render(<ShortcutsTab view={view()} run={run} />);
    const other = vi.fn();
    window.addEventListener("keydown", other);
    await start(/^Find: /);
    press({ key: "Escape" });
    window.removeEventListener("keydown", other);
    expect(other).not.toHaveBeenCalled();
    expect(screen.queryByText("Type the new keys…")).toBeNull();
    expect(setShortcuts).not.toHaveBeenCalled();
  });

  it("refuses keys the OS owns and keeps waiting", async () => {
    document.documentElement.dataset.platform = "mac";
    render(<ShortcutsTab view={view()} run={run} />);
    await start(/^Find: /);
    press({ key: "q", code: "KeyQ", metaKey: true });
    expect((await screen.findByRole("alert")).textContent).toMatch(/quits an app/);
    expect(screen.getByText("Type the new keys…")).toBeTruthy();
    expect(setShortcuts).not.toHaveBeenCalled();
  });

  it("asks before swapping with a shortcut that has the keys", async () => {
    document.documentElement.dataset.platform = "mac";
    render(<ShortcutsTab view={view()} run={run} />);
    await start(/^Find: /);
    press({ key: "n", code: "KeyN", metaKey: true });
    expect(await screen.findByText("Already used by New Project. Swap them?")).toBeTruthy();
    expect(setShortcuts).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Swap" }));
    expect(setShortcuts).toHaveBeenCalledWith({
      search: { key: "n", mod: true },
      "new-project": { key: "f", mod: true },
    });
  });

  it("Cancel on the swap question saves nothing; Reset and Reset All show only when changed", async () => {
    document.documentElement.dataset.platform = "mac";
    render(<ShortcutsTab view={view()} run={run} />);
    await start(/^Find: /);
    press({ key: "n", code: "KeyN", metaKey: true });
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(setShortcuts).not.toHaveBeenCalled();
    cleanup();
    render(<ShortcutsTab view={view({ search: { key: "g", mod: true } })} run={run} />);
    expect(screen.getByRole("button", { name: /^Find: ⌘G/ })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Reset Find to ⌘F" }));
    await waitFor(() => expect(setShortcuts).toHaveBeenCalledWith({}));
    setShortcuts.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Reset All" }));
    await waitFor(() => expect(setShortcuts).toHaveBeenCalledWith({}));
  });
});
