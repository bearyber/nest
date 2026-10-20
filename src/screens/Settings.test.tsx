// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { SettingsView } from "../lib/types";

vi.mock("../lib/commands", () => ({
  getSettings: vi.fn(async () => ({ settings: { theme: "system" }, settingsError: null }) as unknown as SettingsView),
  settingsReady: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
const closeWindow = vi.fn(async () => {});
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "settings", close: closeWindow }) }));
vi.mock("../lib/theme", () => ({ applyTheme: vi.fn() }));
// The tabs themselves are tested on their own.
vi.mock("./settings/AboutTab", () => ({ default: () => null }));
vi.mock("./settings/AppearanceTab", () => ({ default: () => null }));
vi.mock("./settings/CodesTab", () => ({ default: () => null }));
vi.mock("./settings/DevicesTab", () => ({ default: () => null }));
vi.mock("./settings/GeneralTab", () => ({ default: () => null }));
vi.mock("./settings/KitsTab", () => ({ default: () => null }));
vi.mock("./settings/SpacesTab", () => ({ default: () => null }));

import Settings from "./Settings";

afterEach(() => {
  cleanup();
  closeWindow.mockClear();
  delete document.documentElement.dataset.platform;
});

describe("Settings", () => {
  it("has no DaVinci Resolve tab: Resolve is set up in each template", async () => {
    render(<Settings />);
    expect(await screen.findByRole("button", { name: "General" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Devices" })).toBeTruthy();
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
