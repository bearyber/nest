// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import CopiedNote, { copiedNoteText } from "./CopiedNote";

const orig = { deviceId: "1", name: "Studio Mac", os: "macos", letter: "M", guessed: false };

afterEach(cleanup);

function setup(over: Partial<Parameters<typeof CopiedNote>[0]> = {}) {
  const save = vi.fn();
  const use = vi.fn();
  render(
    <CopiedNote
      initialName="Bernard's MacBook"
      mac
      clash={[]}
      letter="M"
      suggested="B"
      onSave={save}
      onUseLetter={use}
      {...over}
    />,
  );
  return { save, use };
}

describe("CopiedNote", () => {
  it("says what happened in plain words", () => {
    expect(copiedNoteText(true)).toBe(
      "This Mac was copied from another Mac, so Nest has given it its own place in your list. Give it a name:",
    );
    expect(copiedNoteText(false)).toContain("This PC was copied from another PC");
  });

  it("starts with the system's name and saves it with one button", () => {
    const { save } = setup();
    const box = screen.getByLabelText("Name for this computer") as HTMLInputElement;
    expect(box.value).toBe("Bernard's MacBook");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith("Bernard's MacBook");
  });

  it("won't save an empty name, and nothing is saved by itself", () => {
    const { save } = setup();
    fireEvent.change(screen.getByLabelText("Name for this computer"), { target: { value: "   " } });
    expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(true);
    expect(save).not.toHaveBeenCalled();
  });

  it("offers a free letter in the same note when the letter is the original's", () => {
    const { use } = setup({ clash: [orig] });
    expect(screen.getByText(/Your Studio Mac and this Mac both use the letter M/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Use B" }));
    expect(use).toHaveBeenCalledWith("B");
  });

  it("shows no letter line when the letters differ", () => {
    setup();
    expect(screen.queryByRole("button", { name: /^Use / })).toBeNull();
  });
});
