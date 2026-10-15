// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

const complete = vi.fn<(...args: unknown[]) => Promise<object>>(async () => ({}));
const pickTemplate = vi.fn(async (): Promise<string | null> => "D:/Work/Acme Job");
let onDrop: ((e: { payload: { type: string; paths: string[] } }) => void) | null = null;

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: async (cb: typeof onDrop) => {
      onDrop = cb;
      return () => {};
    },
  }),
}));
vi.mock("../lib/commands", () => ({
  firstRunPickFolder: vi.fn(async () => ({ path: "D:/Jobs", projects: 3 })),
  firstRunCreateFolder: vi.fn(),
  firstRunPickTemplateFolder: () => pickTemplate(),
  getSettings: vi.fn(async () => ({ suggestedMarker: "W" })),
  completeFirstRun: (...args: unknown[]) => complete(...args),
}));

import FirstRun from "./FirstRun";

beforeEach(() => {
  complete.mockClear();
  onDrop = null;
});
afterEach(cleanup);

async function toSecondScreen() {
  fireEvent.click(screen.getByRole("button", { name: "Choose folder…" }));
  expect(await screen.findByText("Found 3 projects")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
  expect(await screen.findByText("Got a folder you copy for every job?")).toBeTruthy();
}

describe("FirstRun", () => {
  it("is two steps: the jobs folder (with a welcome line), then your own folder", async () => {
    render(<FirstRun onDone={vi.fn()} />);
    expect(screen.getByText("Welcome to Nest")).toBeTruthy();
    expect(screen.getByText("Where do your jobs live?")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Continue" }) as HTMLButtonElement).disabled).toBe(true);
    await toSecondScreen();
  });

  it("Skip finishes without a template, with the default spaces and the suggested letter", async () => {
    const done = vi.fn();
    render(<FirstRun onDone={done} />);
    await toSecondScreen();
    await waitFor(() => expect(screen.getByRole("button", { name: "Skip, use Nest's templates" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Skip, use Nest's templates" }));
    await waitFor(() => expect(done).toHaveBeenCalled());
    expect(complete).toHaveBeenCalledWith("D:/Jobs", ["Work", "Personal"], ["Personal"], [], "W", null);
  });

  it("a chosen folder shows its name and is sent on Finish", async () => {
    const done = vi.fn();
    render(<FirstRun onDone={done} />);
    await toSecondScreen();
    expect((screen.getByRole("button", { name: "Finish" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Choose a folder…" }));
    expect(await screen.findByText("Acme Job")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Change" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => expect(done).toHaveBeenCalled());
    expect(complete).toHaveBeenCalledWith("D:/Jobs", ["Work", "Personal"], ["Personal"], [], "W", "D:/Work/Acme Job");
  });

  it("a dropped folder is used too", async () => {
    const done = vi.fn();
    render(<FirstRun onDone={done} />);
    await toSecondScreen();
    await waitFor(() => expect(onDrop).not.toBeNull());
    act(() => onDrop?.({ payload: { type: "drop", paths: ["C:\\Clients\\Beta Job"] } }));
    expect(await screen.findByText("Beta Job")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Finish" }));
    await waitFor(() => expect(done).toHaveBeenCalled());
    expect(complete.mock.calls[0][5]).toBe("C:\\Clients\\Beta Job");
  });
});
