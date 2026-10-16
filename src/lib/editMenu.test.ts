// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";

let handler: ((e: { payload: string }) => Promise<void>) | null = null;
let focused = true;
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (_name: string, h: (e: { payload: string }) => Promise<void>) => {
    handler = h;
    return () => {};
  },
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isFocused: async () => focused }),
}));

import { EDIT_STEP_EVENT, inTextBox, listenEditMenu } from "./editMenu";

describe("Edit → Undo / Redo from the menu bar", () => {
  const steps: string[] = [];
  const onStep = (e: Event) => steps.push((e as CustomEvent<string>).detail);
  afterEach(() => {
    steps.length = 0;
    window.removeEventListener(EDIT_STEP_EVENT, onStep);
    document.body.innerHTML = "";
    focused = true;
  });

  it("hands Undo / Redo to the screen in the focused window only", async () => {
    window.addEventListener(EDIT_STEP_EVENT, onStep);
    listenEditMenu();
    await vi.waitFor(() => expect(handler).not.toBeNull());
    await handler!({ payload: "undo" });
    await handler!({ payload: "redo" });
    await handler!({ payload: "new-project" });
    focused = false;
    await handler!({ payload: "undo" });
    expect(steps).toEqual(["undo", "redo"]);
  });

  it("knows a text box from a checkbox", () => {
    document.body.innerHTML =
      '<input id="a"><input id="b" type="checkbox"><textarea id="c"></textarea><select id="d"></select>';
    const at = (id: string) => document.getElementById(id);
    expect(inTextBox(at("a"))).toBe(true);
    expect(inTextBox(at("b"))).toBe(false);
    expect(inTextBox(at("c"))).toBe(true);
    expect(inTextBox(at("d"))).toBe(false);
    expect(inTextBox(document.body)).toBe(false);
  });
});
