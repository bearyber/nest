// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isListKey } from "./listKeys";

// The main window's list keys, as Main.tsx wires them: Enter opens, arrows move the selection.
let open: ReturnType<typeof vi.fn<() => void>>;
let move: ReturnType<typeof vi.fn<() => void>>;
const onKey = (e: KeyboardEvent) => {
  if (!isListKey(e)) return;
  if (e.key === "Enter") open();
  else if (e.key === "ArrowDown" || e.key === "ArrowUp") move();
};

const press = (el: Element, key: string) => el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));

beforeEach(() => {
  open = vi.fn();
  move = vi.fn();
  document.body.innerHTML = `
    <nav><button id="space">Acme</button></nav>
    <main id="main">
      <header><button id="new">New</button><input id="search" type="search" /></header>
      <div id="list" role="grid" tabindex="0"><div role="row">A</div></div>
      <div role="status"><button id="restart">Restart to update</button></div>
    </main>
    <div role="dialog" id="dlg" tabindex="-1"><span id="inside">text</span></div>
    <div id="menu" role="menu"><div role="menuitem" id="item" tabindex="-1">Open</div></div>`;
  window.addEventListener("keydown", onKey);
});

afterEach(() => {
  window.removeEventListener("keydown", onKey);
  document.body.innerHTML = "";
});

const $ = (id: string) => document.getElementById(id)!;

describe("isListKey", () => {
  it("Enter on a focused button is the button's, not 'open the folder'", () => {
    for (const id of ["new", "restart", "space"]) {
      $(id).focus();
      press($(id), "Enter");
      press($(id), " ");
    }
    expect(open).not.toHaveBeenCalled();
  });

  it("arrows on a sidebar button don't move the project selection", () => {
    $("space").focus();
    press($("space"), "ArrowDown");
    press($("space"), "ArrowUp");
    expect(move).not.toHaveBeenCalled();
  });

  it("Enter and arrows with focus on the list (or nothing focused) still work", () => {
    $("list").focus();
    press($("list"), "Enter");
    press($("list"), "ArrowDown");
    press(document.body, "Enter");
    expect(open).toHaveBeenCalledTimes(2);
    expect(move).toHaveBeenCalledTimes(1);
  });

  it("leaves text boxes, menus and dialogs alone", () => {
    press($("search"), "ArrowDown");
    press($("item"), "Enter");
    press($("inside"), "Enter");
    expect(open).not.toHaveBeenCalled();
    expect(move).not.toHaveBeenCalled();
  });

  it("skips a key something else already handled", () => {
    $("list").addEventListener("keydown", (e) => e.preventDefault());
    press($("list"), "Enter");
    expect(open).not.toHaveBeenCalled();
  });
});
