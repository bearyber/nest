// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

const complete = vi.fn(async () => ({}));
vi.mock("../lib/commands", () => ({
  listTemplates: vi.fn(async () => [
    { key: "builtin:grading-mv", id: "grading-mv", name: "Grading: MV", version: 3, description: "", builtIn: true, superseded: false, hidden: false, folder: "" },
    { key: "builtin:blank", id: "blank", name: "Blank", version: 3, description: "", builtIn: true, superseded: false, hidden: false, folder: "" },
  ]),
  firstRunPickFolder: vi.fn(async () => ({ path: "D:/Jobs", projects: 3 })),
  firstRunCreateFolder: vi.fn(),
  getSettings: vi.fn(async () => ({ suggestedMarker: "W" })),
  completeFirstRun: (...args: unknown[]) => complete(...(args as [])),
}));

import FirstRun from "./FirstRun";

afterEach(cleanup);

describe("FirstRun", () => {
  it("walks folder → templates → spaces → letter and saves the choices", async () => {
    const done = vi.fn();
    render(<FirstRun onDone={done} />);
    fireEvent.click(screen.getByRole("button", { name: "Get started" }));

    const cont = screen.getByRole("button", { name: "Continue" }) as HTMLButtonElement;
    expect(cont.disabled).toBe(true); // no folder yet
    fireEvent.click(screen.getByRole("button", { name: "Choose folder…" }));
    expect(await screen.findByText("Found 3 projects")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    // Untick Blank.
    fireEvent.click(await screen.findByRole("checkbox", { name: /Blank/ }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));

    // The suggested letter is filled in; J is refused.
    const letter = (await screen.findByLabelText("Letter for this computer")) as HTMLInputElement;
    await waitFor(() => expect(letter.value).toBe("W"));
    fireEvent.change(letter, { target: { value: "j" } });
    expect((screen.getByRole("button", { name: "Start using Nest" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(letter, { target: { value: "p" } });

    fireEvent.click(screen.getByRole("button", { name: "Start using Nest" }));
    await waitFor(() => expect(done).toHaveBeenCalled());
    expect(complete).toHaveBeenCalledWith("D:/Jobs", ["Work", "Personal"], ["Personal"], ["blank"], "P");
  });
});
