// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { SettingsView } from "../lib/types";

vi.mock("../lib/commands", () => ({
  getSettings: vi.fn(async () => ({ settings: { theme: "system" }, settingsError: null }) as unknown as SettingsView),
  settingsReady: vi.fn(async () => {}),
}));
const handlers = new Map<string, (e: { payload?: unknown }) => void>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, cb: (e: { payload?: unknown }) => void) => {
    handlers.set(name, cb);
    return () => {};
  }),
}));
const closeWindow = vi.fn(async () => {});
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "settings", close: closeWindow }) }));
vi.mock("../lib/theme", () => ({ applyTheme: vi.fn() }));
// The tabs themselves are tested on their own.
vi.mock("./settings/AboutTab", () => ({ default: () => null }));
vi.mock("./settings/AppearanceTab", () => ({ default: () => null }));
vi.mock("./settings/CodesTab", () => ({
  default: ({ goTo }: { goTo?: (t: "computers") => void }) => <button onClick={() => goTo?.("computers")}>Change in Computers</button>,
}));
vi.mock("./settings/DevicesTab", () => ({
  default: ({ focusLetter }: { focusLetter?: boolean }) => <div data-testid="computers" data-focus={String(!!focusLetter)} />,
}));
vi.mock("./settings/GeneralTab", () => ({ default: () => null }));
vi.mock("./settings/KitsTab", () => ({ default: () => null }));
vi.mock("./settings/SpacesTab", () => ({ default: () => null }));

import Settings, { startTab } from "./Settings";

afterEach(() => {
  cleanup();
  closeWindow.mockClear();
  delete document.documentElement.dataset.platform;
});

describe("Settings", () => {
  it("opens on the asked-for tab, and a close (hide) sends it back to General", async () => {
    render(<Settings />);
    await screen.findByRole("button", { name: "General" });
    await vi.waitFor(() => expect(handlers.has("settings-reset")).toBe(true));
    act(() => handlers.get("settings-tab")?.({ payload: "shortcuts" }));
    expect(screen.getByRole("button", { name: "Shortcuts" }).getAttribute("aria-current")).toBe("page");
    act(() => handlers.get("settings-reset")?.({}));
    expect(screen.getByRole("button", { name: "General" }).getAttribute("aria-current")).toBe("page");
  });

  it("the Job codes link switches to Computers", async () => {
    render(<Settings />);
    fireEvent.click(await screen.findByRole("button", { name: "Job codes" }));
    fireEvent.click(screen.getByRole("button", { name: "Change in Computers" }));
    expect(screen.getByRole("button", { name: "Computers" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByTestId("computers").getAttribute("data-focus")).toBe("false");
  });

  it("a request for Computers (the same-letter note) asks for the letter to be focused", async () => {
    render(<Settings />);
    await screen.findByRole("button", { name: "General" });
    await vi.waitFor(() => expect(handlers.has("settings-tab")).toBe(true));
    act(() => handlers.get("settings-tab")?.({ payload: "computers" }));
    expect(screen.getByTestId("computers").getAttribute("data-focus")).toBe("true");
  });

  it("has no DaVinci Resolve tab: Resolve is set up in each template", async () => {
    render(<Settings />);
    expect(await screen.findByRole("button", { name: "General" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Computers" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Resolve/ })).toBeNull();
  });

  it("Windows: Escape and Ctrl+W close the window, but not Escape while typing", async () => {
    document.documentElement.dataset.platform = "win";
    render(<Settings />);
    await screen.findByRole("button", { name: "General" });
    const box = document.createElement("input");
    document.body.appendChild(box);
    fireEvent.keyDown(box, { key: "Escape" });
    expect(closeWindow).not.toHaveBeenCalled();
    box.remove();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(closeWindow).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(window, { key: "w", code: "KeyW", ctrlKey: true });
    expect(closeWindow).toHaveBeenCalledTimes(2);
  });

  it("macOS: Escape doesn't close Settings (⌘W is in the menu bar)", async () => {
    document.documentElement.dataset.platform = "mac";
    render(<Settings />);
    await screen.findByRole("button", { name: "General" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(closeWindow).not.toHaveBeenCalled();
  });
});

describe("startTab", () => {
  it("opens on Computers for the same-letter note's button, on Shortcuts for the help menu", () => {
    expect(startTab("computers")).toBe("computers");
    expect(startTab("codes")).toBe("codes");
    expect(startTab("shortcuts")).toBe("shortcuts");
    expect(startTab(null)).toBe("general");
    expect(startTab("anything else")).toBe("general");
  });
});
