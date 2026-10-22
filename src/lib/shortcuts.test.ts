// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import {
  SHORTCUTS,
  closesSettings,
  findShortcut,
  installBrowserKeyGuard,
  isBrowserKey,
  matchesCombo,
  type Combo,
  type KeyLike,
} from "./shortcuts";

/** A key press: letters get their physical key too, like a real keyboard. */
function key(k: string, mods: Partial<Omit<KeyLike, "key">> & { altGr?: boolean } = {}): KeyLike {
  const { altGr, ...rest } = mods;
  return {
    key: k,
    code: /^[a-z]$/i.test(k) ? `Key${k.toUpperCase()}` : "",
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    getModifierState: (m: string) => m === "AltGraph" && altGr === true,
    ...rest,
  };
}
const id = (e: KeyLike, platform: "mac" | "win") => findShortcut(e, undefined, platform)?.shortcut.id ?? null;

describe("shortcut matching", () => {
  it("uses ⌘ on macOS and Ctrl on Windows", () => {
    expect(id(key("n", { metaKey: true }), "mac")).toBe("new-project");
    expect(id(key("n", { ctrlKey: true }), "win")).toBe("new-project");
    expect(id(key("n", { ctrlKey: true }), "mac")).toBeNull();
    expect(id(key("n", { metaKey: true }), "win")).toBeNull();
    expect(id(key(",", { ctrlKey: true }), "win")).toBe("open-settings");
  });

  it("needs the exact modifiers: no extra Shift, Alt or Ctrl", () => {
    expect(id(key("r", { ctrlKey: true }), "win")).toBe("rescan");
    expect(id(key("r", { ctrlKey: true, altKey: true }), "win")).toBeNull();
    expect(id(key("R", { ctrlKey: true, shiftKey: true }), "win")).toBeNull();
    expect(id(key("K", { ctrlKey: true, shiftKey: true }), "win")).toBeNull();
    expect(id(key("N", { metaKey: true, shiftKey: true }), "mac")).toBeNull();
    // ⌃⌘F is macOS full screen, not Find.
    expect(id(key("f", { metaKey: true }), "mac")).toBe("search");
    expect(id(key("f", { metaKey: true, ctrlKey: true }), "mac")).toBeNull();
    // F5 alone refreshes; Ctrl+F5 doesn't.
    expect(id(key("F5"), "win")).toBe("rescan");
    expect(id(key("F5", { ctrlKey: true }), "win")).toBeNull();
  });

  it("finds ⌥⌘I by its physical key (⌥ turns I into another character on a Mac)", () => {
    expect(id({ ...key("ˆ", { metaKey: true, altKey: true }), code: "KeyI" }, "mac")).toBe("toggle-inspector");
    expect(id(key("i", { ctrlKey: true, altKey: true }), "win")).toBe("toggle-inspector");
  });

  it("never treats AltGr (typing a character) as Ctrl+Alt on Windows", () => {
    expect(id(key("i", { ctrlKey: true, altKey: true, altGr: true }), "win")).toBeNull();
    expect(id(key("r", { ctrlKey: true, altKey: true, altGr: true }), "win")).toBeNull();
  });

  it("Open, Copy Path and the sidebar have their keys on each OS", () => {
    expect(id(key("o", { metaKey: true }), "mac")).toBe("open-project");
    expect(id(key("o", { ctrlKey: true }), "win")).toBe("open-project");
    // ⌥⌘C on a Mac (⌥ turns C into ç: the physical key says which letter it is).
    expect(id({ ...key("ç", { metaKey: true, altKey: true }), code: "KeyC" }, "mac")).toBe("copy-path");
    expect(id(key("c", { ctrlKey: true, shiftKey: true }), "win")).toBe("copy-path");
    // Plain Ctrl+C is copy, and Ctrl+Alt+C (AltGr on many keyboards) is nothing.
    expect(id(key("c", { ctrlKey: true }), "win")).toBeNull();
    expect(id(key("c", { ctrlKey: true, altKey: true, altGr: true }), "win")).toBeNull();
    expect(id(key("c", { metaKey: true }), "mac")).toBeNull();
    // ⌃⌘S hides / shows the sidebar on a Mac; ⌘S alone is not it, and ⌘B still works.
    expect(id(key("s", { metaKey: true, ctrlKey: true }), "mac")).toBe("toggle-sidebar");
    expect(id(key("s", { metaKey: true }), "mac")).toBeNull();
    expect(id(key("b", { metaKey: true }), "mac")).toBe("toggle-sidebar");
    expect(id(key("b", { ctrlKey: true }), "win")).toBe("toggle-sidebar");
    expect(id(key("s", { ctrlKey: true }), "win")).toBeNull();
  });

  it("knows ⌘K / Ctrl+K as a second way to Find, and Undo / Redo per OS", () => {
    expect(id(key("k", { metaKey: true }), "mac")).toBe("search");
    expect(findShortcut(key("k", { metaKey: true }), undefined, "mac")?.combo.menu).toBeFalsy();
    expect(findShortcut(key("f", { metaKey: true }), undefined, "mac")?.combo.menu).toBe(true);
    expect(id(key("y", { ctrlKey: true }), "win")).toBe("redo");
    expect(id(key("Z", { ctrlKey: true, shiftKey: true }), "win")).toBe("redo");
    expect(id(key("z", { metaKey: true, shiftKey: true }), "mac")).toBe("redo");
    expect(id(key("y", { metaKey: true }), "mac")).toBeNull();
  });

  it("only looks at the shortcuts asked for", () => {
    expect(findShortcut(key("z", { ctrlKey: true }), ["new-project"], "win")).toBeNull();
    expect(matchesCombo(key("Enter", { ctrlKey: true }), { key: "Enter", mod: true }, "win")).toBe(true);
    expect(matchesCombo(key("Enter", { ctrlKey: true, shiftKey: true }), { key: "Enter", mod: true }, "win")).toBe(false);
  });
});

describe("the macOS menu bar uses the same keys (src-tauri/src/menu.rs)", () => {
  const menu = Object.values(
    import.meta.glob<string>("../../src-tauri/src/menu.rs", { eager: true, query: "?raw", import: "default" }),
  )[0];
  const ids = Object.fromEntries([...menu.matchAll(/pub const (\w+): &str = "([^"]+)";/g)].map((m) => [m[1], m[2]]));
  const accelerators = new Map(
    [...menu.matchAll(/(?:item|disabled)\(\s*(\w+),\s*"[^"]*",\s*Some\("([^"]+)"\)/g)].map((m) => [ids[m[1]], m[2]]),
  );
  const accelerator = (c: Combo) =>
    [c.ctrl && "Ctrl", c.mod && "CmdOrCtrl", c.shift && "Shift", c.alt && "Alt", c.key.length === 1 ? c.key.toUpperCase() : c.key]
      .filter(Boolean)
      .join("+");

  it("every menu item's accelerator is the shortcut's macOS key, and the other way round", () => {
    expect(accelerators.size).toBeGreaterThan(5);
    for (const [menuId, acc] of accelerators) {
      const s = SHORTCUTS.find((x) => x.id === menuId);
      expect(s, menuId).toBeTruthy();
      expect(s!.mac.filter((c) => c.menu).map(accelerator), menuId).toEqual([acc]);
    }
    // Close Window is the menu's predefined item (⌘W), not one of ours.
    for (const s of SHORTCUTS.filter((x) => x.id !== "close-window" && x.mac.some((c) => c.menu))) {
      expect(accelerators.has(s.id), s.id).toBe(true);
    }
    expect(accelerators.get("search")).toBe("CmdOrCtrl+F");
  });
});

describe("Windows: the webview's own keys are blocked", () => {
  it("blocks reload, print, find and zoom", () => {
    for (const e of [
      key("F5"),
      key("F5", { ctrlKey: true }),
      key("r", { ctrlKey: true }),
      key("R", { ctrlKey: true, shiftKey: true }),
      key("p", { ctrlKey: true }),
      key("f", { ctrlKey: true }),
      key("g", { ctrlKey: true }),
      key("o", { ctrlKey: true }),
      key("F3"),
      key("F7"),
      key("=", { ctrlKey: true }),
      key("+", { ctrlKey: true, shiftKey: true }),
      key("-", { ctrlKey: true }),
      key("0", { ctrlKey: true }),
    ]) {
      expect(isBrowserKey(e, "win"), JSON.stringify(e)).toBe(true);
    }
  });

  it("leaves editing keys, AltGr and macOS alone", () => {
    for (const e of [
      key("c", { ctrlKey: true }),
      key("v", { ctrlKey: true }),
      key("x", { ctrlKey: true }),
      key("z", { ctrlKey: true }),
      key("y", { ctrlKey: true }),
      key("a", { ctrlKey: true }),
      key("ArrowLeft", { ctrlKey: true }),
      key("Backspace", { ctrlKey: true }),
      key("r"),
      key("0"),
      key("r", { ctrlKey: true, altKey: true, altGr: true }),
    ]) {
      expect(isBrowserKey(e, "win"), JSON.stringify(e)).toBe(false);
    }
    expect(isBrowserKey(key("r", { metaKey: true }), "mac")).toBe(false);
    expect(isBrowserKey(key("F5"), "mac")).toBe(false);
  });

  it("the guard stops them in the window (keys and Ctrl+wheel), on Windows only", () => {
    const undo = installBrowserKeyGuard(window, "win");
    const press = (init: KeyboardEventInit) => {
      const e = new KeyboardEvent("keydown", { cancelable: true, ...init });
      window.dispatchEvent(e);
      return e.defaultPrevented;
    };
    expect(press({ key: "F5" })).toBe(true);
    expect(press({ key: "r", code: "KeyR", ctrlKey: true })).toBe(true);
    expect(press({ key: "c", code: "KeyC", ctrlKey: true })).toBe(false);
    const wheel = new WheelEvent("wheel", { ctrlKey: true, cancelable: true });
    window.dispatchEvent(wheel);
    expect(wheel.defaultPrevented).toBe(true);
    undo();
    expect(press({ key: "F5" })).toBe(false);
    installBrowserKeyGuard(window, "mac")();
  });
});

describe("Windows: closing the Settings window from the keyboard", () => {
  const ev = (k: string, mods: Partial<KeyLike> = {}, target: EventTarget | null = document.body, defaultPrevented = false) => ({
    ...key(k, mods),
    target,
    defaultPrevented,
  });

  it("Escape and Ctrl+W close it", () => {
    expect(closesSettings(ev("Escape"), "win")).toBe(true);
    expect(closesSettings(ev("w", { ctrlKey: true }), "win")).toBe(true);
  });

  it("not while typing, not when something inside used Escape, and never on macOS", () => {
    const input = document.createElement("input");
    const select = document.createElement("select");
    expect(closesSettings(ev("Escape", {}, input), "win")).toBe(false);
    expect(closesSettings(ev("Escape", {}, select), "win")).toBe(false);
    expect(closesSettings(ev("Escape", {}, document.body, true), "win")).toBe(false);
    expect(closesSettings(ev("Escape", { shiftKey: true }), "win")).toBe(false);
    expect(closesSettings(ev("W", { ctrlKey: true, shiftKey: true }), "win")).toBe(false);
    expect(closesSettings(ev("Escape"), "mac")).toBe(false);
    expect(closesSettings(ev("w", { metaKey: true }), "mac")).toBe(false);
  });
});
