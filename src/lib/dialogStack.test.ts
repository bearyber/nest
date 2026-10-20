// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { hasOpenDialog, isTopDialog, pushDialog, topDialog } from "./dialogStack";

describe("dialog stack", () => {
  it("only the dialog on top acts; closing it hands over to the one below", () => {
    const editor = document.createElement("div");
    const sheet = document.createElement("div");
    expect(hasOpenDialog()).toBe(false);
    expect(isTopDialog(null)).toBe(true);
    const closeEditor = pushDialog(editor);
    const closeSheet = pushDialog(sheet);
    expect(topDialog()).toBe(sheet);
    expect(isTopDialog(sheet)).toBe(true);
    expect(isTopDialog(editor)).toBe(false);
    // A dialog not on the stack waits while another is open.
    expect(isTopDialog(null)).toBe(false);
    closeSheet();
    expect(isTopDialog(editor)).toBe(true);
    closeEditor();
    expect(hasOpenDialog()).toBe(false);
  });

  it("a dialog inside another goes on top even when it was added first", () => {
    const outer = document.createElement("div");
    const inner = document.createElement("div");
    outer.appendChild(inner);
    const a = pushDialog(inner);
    const b = pushDialog(outer);
    expect(topDialog()).toBe(inner);
    a();
    b();
    expect(hasOpenDialog()).toBe(false);
  });
});
