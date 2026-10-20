// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import WhatsNew from "./WhatsNew";
import { updateNoteLines } from "../lib/types";

afterEach(cleanup);

describe("WhatsNew", () => {
  it("shows the title and lines as plain text, and OK closes it", () => {
    const ok = vi.fn();
    render(<WhatsNew note={{ version: "0.5.0", lines: ["One <b>thing</b>", "Two"] }} onOk={ok} />);
    expect(screen.getByRole("heading", { name: "What’s new in Nest 0.5.0" })).toBeTruthy();
    expect(screen.getAllByRole("listitem").map((l) => l.textContent)).toEqual(["One <b>thing</b>", "Two"]);
    fireEvent.click(screen.getByRole("button", { name: "OK" }));
    expect(ok).toHaveBeenCalledTimes(1);
  });

  it("starts on a focusable OK, keeps Tab inside, and Return still closes it", () => {
    const ok = vi.fn();
    render(
      <>
        <button>Behind</button>
        <WhatsNew note={{ version: "0.5.0", lines: ["One"] }} onOk={ok} />
      </>,
    );
    const button = screen.getByRole("button", { name: "OK" });
    expect(document.activeElement).toBe(button);
    expect(screen.getByText("Behind").hasAttribute("inert")).toBe(true);
    fireEvent.keyDown(button, { key: "Tab" });
    expect(document.activeElement).toBe(button);
    fireEvent.keyDown(button, { key: "Enter" });
    expect(ok).toHaveBeenCalledTimes(1);
  });
});

describe("updateNoteLines", () => {
  it("splits, strips bullets, drops blanks and caps at 4", () => {
    const notes = "- one\n* two\r\n\n  three\n- four\n- five";
    expect(updateNoteLines(notes)).toEqual(["one", "two", "three", "four"]);
    expect(updateNoteLines(null)).toEqual([]);
  });
});
