// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import type { SettingsView } from "../lib/types";

vi.mock("../lib/commands", () => ({
  getSettings: vi.fn(async () => ({ settings: { theme: "system" }, settingsError: null }) as unknown as SettingsView),
  settingsReady: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
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

afterEach(cleanup);

describe("Settings", () => {
  it("has no DaVinci Resolve tab: Resolve is set up in each template", async () => {
    render(<Settings />);
    expect(await screen.findByRole("button", { name: "General" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Devices" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Resolve/ })).toBeNull();
  });
});
