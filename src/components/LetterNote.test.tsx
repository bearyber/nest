// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import LetterNote, { letterNoteText } from "./LetterNote";

const pc = { deviceId: "1", name: "PC", os: "windows", letter: "W", guessed: false };

afterEach(cleanup);

describe("letterNoteText", () => {
  it("says what happened in plain words", () => {
    expect(letterNoteText([pc], "W", true)).toBe("Your PC and this Mac both use W. Change this computer's letter?");
    expect(letterNoteText([{ ...pc, name: "Mac" }], "W", false)).toBe(
      "Your Mac and this PC both use W. Change this computer's letter?",
    );
  });

  it("says 'seem to' when another computer's letter is only judged from its codes", () => {
    expect(letterNoteText([{ ...pc, guessed: true }], "W", true)).toContain("seem to both use W");
  });

  it("lists several computers", () => {
    const text = letterNoteText([pc, { ...pc, deviceId: "2", name: "서울 맥" }], "W", true);
    expect(text).toContain("Your PC and 서울 맥 and this Mac both use W");
  });
});

describe("LetterNote", () => {
  it("offers the change as one button and 'Later' as another; nothing happens by itself", () => {
    const change = vi.fn();
    const later = vi.fn();
    const keep = vi.fn();
    render(<LetterNote clash={[pc]} letter="W" mac onChange={change} onKeep={keep} onLater={later} />);
    expect(change).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Change letter…" }));
    expect(change).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Keep W" }));
    expect(keep).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(later).toHaveBeenCalledTimes(1);
  });

  it("only the sentence is a live region (not the buttons), and it stays the same element when re-rendered", () => {
    const props = { letter: "W", mac: true, onChange: () => {}, onKeep: () => {}, onLater: () => {} };
    const { rerender } = render(<LetterNote clash={[pc]} {...props} />);
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("both use W");
    expect(status.querySelector("button")).toBeNull();
    rerender(<LetterNote clash={[{ ...pc }]} {...props} />);
    expect(screen.getByRole("status")).toBe(status);
  });
});
