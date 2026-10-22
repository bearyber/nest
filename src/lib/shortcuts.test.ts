// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import {
  SHORTCUTS,
  checkCombo,
  closesSettings,
  effectiveCombos,
  findShortcut,
  describeCombo,
  formatCombo,
  listedShortcuts,
  planChange,
  planReset,
  recordedCombo,
  setCustomKeys,
  installBrowserKeyGuard,
  installHelpKey,
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
    expect(matchesCombo(key("Enter", { ctrlKey: true, shiftKey: true }), { key: "Enter", mod: true }, "win")).toBe(
      false,
    );
  });
});

describe("the macOS menu bar uses the same keys (src-tauri/src/menu.rs)", () => {
  const menu = Object.values(
    import.meta.glob<string>("../../src-tauri/src/menu.rs", {
      eager: true,
      query: "?raw",
      import: "default",
    }),
  )[0];
  const ids = Object.fromEntries([...menu.matchAll(/pub const (\w+): &str = "([^"]+)";/g)].map((m) => [m[1], m[2]]));
  const accelerators = new Map(
    [...menu.matchAll(/(?:item|disabled)\(\s*(\w+),\s*"[^"]*",\s*Some\("([^"]+)"\)/g)].map((m) => [ids[m[1]], m[2]]),
  );
  const accelerator = (c: Combo) =>
    [
      c.ctrl && "Ctrl",
      c.mod && "CmdOrCtrl",
      c.shift && "Shift",
      c.alt && "Alt",
      c.key.length === 1 ? c.key.toUpperCase() : c.key,
    ]
      .filter(Boolean)
      .join("+");

  it("every menu item's accelerator is the shortcut's macOS key, and the other way round", () => {
    expect(accelerators.size).toBeGreaterThan(5);
    for (const [menuId, acc] of accelerators) {
      const s = SHORTCUTS.find((x) => x.id === menuId);
      expect(s, menuId).toBeTruthy();
      expect(s!.mac.filter((c) => c.menu).map(accelerator), menuId).toEqual([acc]);
    }
    for (const s of SHORTCUTS.filter((x) => x.mac.some((c) => c.menu))) {
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
  const ev = (
    k: string,
    mods: Partial<KeyLike> = {},
    target: EventTarget | null = document.body,
    defaultPrevented = false,
  ) => ({
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

describe("the ids Rust may save (src-tauri/src/shortcut_keys.rs) are the shortcut ids", () => {
  const rust = Object.values(
    import.meta.glob<string>("../../src-tauri/src/shortcut_keys.rs", {
      eager: true,
      query: "?raw",
      import: "default",
    }),
  )[0];
  it("match, in the same order", () => {
    const list = rust.match(/SHORTCUT_IDS: \[&str; \d+\] = \[([^\]]+)\]/)![1];
    const ids = [...list.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect(ids).toEqual(SHORTCUTS.map((s) => s.id));
  });
});

describe("changed keys", () => {
  afterEach(() => setCustomKeys({}));
  const n = { key: "n", mod: true };

  it("a changed key replaces the first key; the extras and the menu flag stay", () => {
    setCustomKeys({ search: { key: "g", mod: true }, rescan: { key: "F6" } });
    expect(id(key("g", { metaKey: true }), "mac")).toBe("search");
    expect(id(key("f", { metaKey: true }), "mac")).toBeNull();
    expect(id(key("k", { metaKey: true }), "mac")).toBe("search");
    expect(findShortcut(key("g", { metaKey: true }), undefined, "mac")?.combo.menu).toBe(true);
    // Refresh keeps F5 as an extra on both OSes.
    expect(id(key("F6"), "win")).toBe("rescan");
    expect(id(key("F5"), "win")).toBe("rescan");
    expect(id(key("r", { ctrlKey: true }), "win")).toBeNull();
    expect(
      effectiveCombos(
        SHORTCUTS.find((s) => s.id === "search")!,
        "win",
      ).map((c) => formatCombo(c, "win")),
    ).toEqual(["Ctrl+G", "Ctrl+K"]);
  });

  it("ignores ids it doesn't know, and no changes means the defaults", () => {
    setCustomKeys({ "from-the-future": { key: "j", mod: true } });
    expect(id(key("j", { ctrlKey: true }), "win")).toBeNull();
    expect(id(key("n", { ctrlKey: true }), "win")).toBe("new-project");
    setCustomKeys(undefined);
    expect(id(key("n", { metaKey: true }), "mac")).toBe("new-project");
  });

  it("shows keys the way each OS writes them", () => {
    expect(formatCombo({ key: "n", mod: true, shift: true }, "mac")).toBe("⇧⌘N");
    expect(formatCombo({ key: "s", mod: true, ctrl: true }, "mac")).toBe("⌃⌘S");
    expect(formatCombo({ key: "c", mod: true, alt: true }, "mac")).toBe("⌥⌘C");
    expect(formatCombo({ key: "Enter", mod: true }, "mac")).toBe("⌘↩");
    expect(formatCombo({ key: "n", mod: true, shift: true }, "win")).toBe("Ctrl+Shift+N");
    expect(formatCombo({ key: "F5" }, "win")).toBe("F5");
    // ? already is a shifted character: shown as "?", not "Shift+?".
    expect(formatCombo({ key: "?", shift: true }, "win")).toBe("?");
    expect(formatCombo({ key: "ArrowUp", mod: true }, "mac")).toBe("⌘↑");
    expect(formatCombo({ key: "ArrowUp", mod: true }, "win")).toBe("Ctrl+↑");
    expect(formatCombo({ key: " ", mod: true }, "win")).toBe("Ctrl+Space");
  });

  it("describes keys in words for a screen reader, by OS", () => {
    expect(describeCombo({ key: "n", mod: true, shift: true }, "mac")).toBe("Shift Command N");
    expect(describeCombo({ key: "s", mod: true, ctrl: true }, "mac")).toBe("Control Command S");
    expect(describeCombo({ key: "c", mod: true, alt: true }, "mac")).toBe("Option Command C");
    expect(describeCombo({ key: "Enter", mod: true }, "mac")).toBe("Command Return");
    expect(describeCombo({ key: "n", mod: true, shift: true }, "win")).toBe("Control Shift N");
    expect(describeCombo({ key: "Enter", mod: true }, "win")).toBe("Control Enter");
    expect(describeCombo({ key: "ArrowUp", mod: true }, "mac")).toBe("Command Up Arrow");
    expect(describeCombo({ key: " ", mod: true }, "win")).toBe("Control Space");
    expect(describeCombo({ key: ",", mod: true }, "mac")).toBe("Command Comma");
    expect(describeCombo({ key: "?", shift: true }, "win")).toBe("Question Mark");
    expect(describeCombo({ key: "F5" }, "win")).toBe("F5");
  });

  it("messages say Option on a Mac and Alt on Windows", () => {
    const shifted = { key: ",", mod: true, shift: true };
    expect(checkCombo("search", shifted, "mac")).toMatch(/Shift and Option only/);
    expect(checkCombo("search", shifted, "win")).toMatch(/Shift and Alt only/);
    expect(checkCombo("search", shifted, "win")).not.toMatch(/Option/);
  });

  it("refuses ⌃⌘D, ⌃F2 and ⌃F3 on a Mac", () => {
    expect(checkCombo("search", { key: "d", mod: true, ctrl: true }, "mac")).toMatch(/looks a word up/);
    expect(checkCombo("search", { key: "F2", ctrl: true }, "mac")).toMatch(/menu bar/);
    expect(checkCombo("search", { key: "F3", ctrl: true }, "mac")).toMatch(/Dock/);
    expect(checkCombo("search", { key: "0", mod: true }, "mac")).toMatch(/Nest window/);
    expect(checkCombo("search", { key: "F2" }, "mac")).toBeNull();
  });

  it("other keyboard layouts: the typed character wins unless Option / Alt is held", () => {
    // French (AZERTY): , and ? are on the M key's place.
    const fr = (k: string, mods: Partial<KeyLike> = {}): KeyLike => ({
      ...key(k, mods),
      code: "KeyM",
    });
    expect(matchesCombo(fr(",", { metaKey: true }), { key: ",", mod: true }, "mac")).toBe(true);
    expect(matchesCombo(fr(",", { metaKey: true }), { key: "m", mod: true }, "mac")).toBe(false);
    expect(matchesCombo(fr("?", { shiftKey: true }), { key: "?", shift: true }, "win")).toBe(true);
    expect(matchesCombo(fr("m", { ctrlKey: true }), { key: "m", mod: true }, "win")).toBe(true);
    expect(recordedCombo(fr(",", { metaKey: true }), "mac")).toEqual({
      combo: { key: ",", mod: true },
    });
    // Option / Alt held: the physical key says which letter (⌥M types µ on a Mac).
    expect(
      matchesCombo(
        { ...key("µ", { metaKey: true, altKey: true }), code: "KeyM" },
        { key: "m", mod: true, alt: true },
        "mac",
      ),
    ).toBe(true);
    expect(
      matchesCombo(
        { ...key("m", { ctrlKey: true, altKey: true }), code: "KeyM" },
        { key: "m", mod: true, alt: true },
        "win",
      ),
    ).toBe(true);
    expect(matchesCombo(fr(",", { metaKey: true, altKey: true }), { key: ",", mod: true, alt: true }, "mac")).toBe(
      false,
    );
  });

  it("Quit is listed on a Mac only; Show Keyboard Shortcuts has F1 and ? on Windows and no key on a Mac", () => {
    expect(listedShortcuts("mac").some((s) => s.id === "quit")).toBe(true);
    expect(listedShortcuts("win").some((s) => s.id === "quit")).toBe(false);
    expect(id(key("F1"), "win")).toBe("keyboard-shortcuts");
    expect(id(key("?", { shiftKey: true }), "win")).toBe("keyboard-shortcuts");
    expect(id(key("?", { shiftKey: true }), "mac")).toBeNull();
    setCustomKeys({ "keyboard-shortcuts": { key: "/", mod: true } });
    const hit = findShortcut(key("/", { metaKey: true }), undefined, "mac");
    expect(hit?.shortcut.id).toBe("keyboard-shortcuts");
    expect(hit?.combo.menu).toBe(true);
  });

  it("AltGr never runs a changed key either", () => {
    setCustomKeys({ search: { key: "g", mod: true } });
    expect(id(key("g", { ctrlKey: true, altKey: true, altGr: true }), "win")).toBeNull();
  });

  describe("what can be recorded", () => {
    it("refuses what would type, what the OS owns, and Alt on Windows", () => {
      expect(checkCombo("search", { key: "g", mod: true }, "mac")).toBeNull();
      expect(checkCombo("search", { key: "F6" }, "win")).toBeNull();
      expect(checkCombo("search", { key: "g" }, "win")).toMatch(/Ctrl/);
      expect(checkCombo("search", { key: "g", shift: true }, "mac")).toMatch(/⌘/);
      expect(checkCombo("search", { key: "g", mod: true, alt: true }, "win")).toMatch(/Alt/);
      expect(checkCombo("search", { key: "Escape", mod: true }, "mac")).toMatch(/letter/);
      expect(checkCombo("search", { key: "1", mod: true, shift: true }, "mac")).toMatch(/Shift/);
      expect(checkCombo("search", { key: "q", mod: true }, "mac")).toMatch(/quits/);
      expect(checkCombo("quit", { key: "q", mod: true }, "mac")).toBeNull();
      expect(checkCombo("search", { key: "h", mod: true }, "mac")).toMatch(/hides/);
      expect(checkCombo("search", { key: "c", mod: true }, "win")).toMatch(/copies/);
      expect(checkCombo("search", { key: "c", mod: true }, "mac")).toMatch(/copies/);
    });

    it("turns key presses into combinations; a modifier alone is not one", () => {
      expect(recordedCombo(key("Meta", { metaKey: true }), "mac")).toBeNull();
      expect(recordedCombo(key("g", { metaKey: true, shiftKey: true }), "mac")).toEqual({
        combo: { key: "g", mod: true, shift: true },
      });
      expect(recordedCombo(key("s", { metaKey: true, ctrlKey: true }), "mac")).toEqual({
        combo: { key: "s", mod: true, ctrl: true },
      });
      expect(recordedCombo(key("g", { ctrlKey: true }), "win")).toEqual({
        combo: { key: "g", mod: true },
      });
      expect(recordedCombo(key("g", { ctrlKey: true, altKey: true, altGr: true }), "win")).toHaveProperty("error");
      expect(recordedCombo(key("g", { metaKey: true }), "win")).toHaveProperty("error");
    });
  });

  describe("giving a shortcut new keys", () => {
    it("saves free keys, and a key equal to the default stores nothing", () => {
      const c = planChange("search", { key: "g", mod: true }, "mac", {});
      expect(c).toEqual({
        kind: "save",
        overrides: { search: { key: "g", mod: true } },
      });
      const back = planChange("search", { key: "f", mod: true }, "mac", {
        search: { key: "g", mod: true },
      });
      expect(back).toEqual({ kind: "save", overrides: {} });
      expect(planChange("search", { key: "f", mod: true }, "mac", {}).kind).toBe("same");
    });

    it("asks to swap when another shortcut has the keys, and swaps both", () => {
      const c = planChange("search", n, "mac", {});
      expect(c.kind).toBe("swap");
      if (c.kind !== "swap") return;
      expect(c.message).toBe("Already used by New Project. Swap them?");
      expect(c.with.id).toBe("new-project");
      expect(c.overrides).toEqual({
        search: { key: "n", mod: true },
        "new-project": { key: "f", mod: true },
      });
    });

    it("never saves a silent duplicate: an extra key, or a swap the other can't take, is refused", () => {
      const extra = planChange("rescan", { key: "k", mod: true }, "mac", {});
      expect(extra.kind).toBe("error");
      // ⌘Q is the Mac's own Quit key: nothing else can have it.
      expect(planChange("new-project", { key: "q", mod: true }, "mac", {}).kind).toBe("error");
      // A shortcut with no keys has nothing to give in exchange.
      expect(planChange("keyboard-shortcuts", n, "mac", {}).kind).toBe("error");
      // ...and the other way round: Quit can't take ⌘N's place, so ⌘N can't be swapped with it.
      expect(planChange("quit", { key: "n", mod: true }, "mac", {}).kind).toBe("error");
    });

    it("Reset puts the default back, asking first when someone else took it", () => {
      const own = { search: { key: "g", mod: true } };
      expect(planReset("search", "mac", own)).toEqual({
        kind: "save",
        overrides: {},
      });
      const taken = {
        search: { key: "g", mod: true },
        rescan: { key: "f", mod: true },
      };
      const c = planReset("search", "mac", taken);
      expect(c.kind).toBe("swap");
      if (c.kind === "swap") expect(c.overrides).toEqual({ rescan: { key: "g", mod: true } });
      // No default key (Mac Show Keyboard Shortcuts): the entry is just removed.
      expect(
        planReset("keyboard-shortcuts", "mac", {
          "keyboard-shortcuts": { key: "j", mod: true },
        }),
      ).toEqual({ kind: "save", overrides: {} });
    });
  });
});

describe("macOS: a menu bar key is left for the menu bar", () => {
  afterEach(() => setCustomKeys({}));

  // In the webview, preventDefault on a key stops it reaching the menu bar: ⌘N did nothing (0.7.2).
  it("the page doesn't swallow a key the Help menu item owns", () => {
    setCustomKeys({ "keyboard-shortcuts": { key: "k", mod: true, shift: true } });
    const stop = installHelpKey(window, "mac");
    const e = new KeyboardEvent("keydown", { key: "k", code: "KeyK", metaKey: true, shiftKey: true, cancelable: true });
    window.dispatchEvent(e);
    stop();
    expect(e.defaultPrevented).toBe(false);
  });
});
