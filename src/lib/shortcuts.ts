// Nest's keyboard shortcuts, in one place (build spec §8). Each has an id, a label, and its keys
// on macOS and on Windows. On macOS the main ones live in the menu bar (src-tauri/src/menu.rs):
// those keys are marked `menu: true`, and menu.rs must use the same accelerators
// (shortcuts.test.ts checks it). Windows has no menu bar, so the page's keydown handlers run them.
//
// Matching is exact: Ctrl+Shift+R is not Ctrl+R, ⌃⌘F (full screen) is not ⌘F, and AltGr (which
// Windows reports as Ctrl+Alt) never counts as a shortcut, since it types characters.
//
// Keys can be changed per computer (Settings → Shortcuts, saved in settings as `shortcuts`). A
// changed key replaces a shortcut's first key; its extra keys (⌘K for Find…) stay. Everything
// reads the effective keys (`effectiveCombos`): the page's key handlers, the Settings list, and
// on macOS the menu bar (menu.rs rebuilds its accelerators from the same saved keys).
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, openSettings } from "./commands";
import { isMac, type Platform } from "./platform";
import type { StoredCombo } from "./types";

export type ShortcutId =
  | "new-project"
  | "open-settings"
  | "rescan"
  | "search"
  | "toggle-sidebar"
  | "toggle-inspector"
  | "open-project"
  | "copy-path"
  | "undo"
  | "redo"
  | "create"
  | "close-window"
  | "quit"
  | "keyboard-shortcuts";

/** One key combination. */
export interface Combo {
  /** `KeyboardEvent.key`: a lower-case letter ("n"), a character (","), or a key name ("F5"). */
  key: string;
  /** ⌘ on macOS, Ctrl on Windows. */
  mod?: boolean;
  shift?: boolean;
  /** ⌥ on macOS, Alt on Windows. */
  alt?: boolean;
  /** ⌃ on macOS (a key of its own there). Not used on Windows, where Ctrl is `mod`. */
  ctrl?: boolean;
  /** macOS: the menu bar item's own accelerator. The menu runs it (lib.rs sends it to the right
   *  window as a `menu` event), so the page only stops the key from doing anything else. */
  menu?: boolean;
}

export interface Shortcut {
  id: ShortcutId;
  label: string;
  mac: Combo[];
  win: Combo[];
  /** False: does nothing while you're typing in a text box. */
  whileTyping?: boolean;
  /** Only exists on this OS (Quit has no key on Windows): not listed on the other. */
  onlyOn?: Platform;
  /** macOS: has a menu bar item even with no default key, so a key you give it is its accelerator. */
  menuItem?: boolean;
}

export const SHORTCUTS: readonly Shortcut[] = [
  {
    id: "new-project",
    label: "New Project",
    mac: [{ key: "n", mod: true, menu: true }],
    win: [{ key: "n", mod: true }],
  },
  {
    id: "open-settings",
    label: "Settings",
    mac: [{ key: ",", mod: true, menu: true }],
    win: [{ key: ",", mod: true }],
  },
  {
    id: "rescan",
    label: "Refresh List",
    mac: [{ key: "r", mod: true, menu: true }, { key: "F5" }],
    win: [{ key: "r", mod: true }, { key: "F5" }],
  },
  {
    id: "search",
    label: "Find",
    // ⌘K / Ctrl+K: a hidden extra, as in many apps.
    mac: [
      { key: "f", mod: true, menu: true },
      { key: "k", mod: true },
    ],
    win: [
      { key: "f", mod: true },
      { key: "k", mod: true },
    ],
  },
  {
    id: "toggle-sidebar",
    label: "Toggle Sidebar",
    // Menu: Hide Sidebar / Show Sidebar. ⌘B is a hidden extra on a Mac (as in other apps).
    mac: [
      { key: "s", mod: true, ctrl: true, menu: true },
      { key: "b", mod: true },
    ],
    win: [{ key: "b", mod: true }],
  },
  {
    id: "toggle-inspector",
    label: "Toggle Details",
    mac: [{ key: "i", mod: true, alt: true, menu: true }],
    win: [{ key: "i", mod: true, alt: true }],
    whileTyping: false,
  },
  {
    // The selected project's folder. Windows: Ctrl+O; no Alt/AltGr combos there.
    id: "open-project",
    label: "Open",
    mac: [{ key: "o", mod: true, menu: true }],
    win: [{ key: "o", mod: true }],
    whileTyping: false,
  },
  {
    // Ctrl+Shift+C on Windows: Ctrl+Alt+C is AltGr on many keyboards.
    id: "copy-path",
    label: "Copy Path",
    mac: [{ key: "c", mod: true, alt: true, menu: true }],
    win: [{ key: "c", mod: true, shift: true }],
    whileTyping: false,
  },
  {
    id: "undo",
    label: "Undo",
    mac: [{ key: "z", mod: true, menu: true }],
    win: [{ key: "z", mod: true }],
  },
  {
    id: "redo",
    label: "Redo",
    mac: [{ key: "z", mod: true, shift: true, menu: true }],
    win: [
      { key: "y", mod: true },
      { key: "z", mod: true, shift: true },
    ],
  },
  {
    id: "create",
    label: "Create (New Project)",
    mac: [{ key: "Enter", mod: true }],
    win: [{ key: "Enter", mod: true }],
  },
  {
    id: "close-window",
    label: "Close Window",
    // macOS: the menu bar's own Close Window item (menu.rs closes the window you're in).
    mac: [{ key: "w", mod: true, menu: true }],
    win: [{ key: "w", mod: true }],
  },
  {
    // The menu bar's Quit Nest. Windows has none: quit from the notification area icon.
    id: "quit",
    label: "Quit Nest",
    mac: [{ key: "q", mod: true, menu: true }],
    win: [],
    onlyOn: "mac",
  },
  {
    // Opens Settings on the Shortcuts tab. Mac: Help → Keyboard Shortcuts (no key until you give
    // it one). Windows: F1 anywhere, and ? when you're not typing.
    id: "keyboard-shortcuts",
    label: "Show Keyboard Shortcuts",
    mac: [],
    win: [{ key: "F1" }, { key: "?", shift: true }],
    menuItem: true,
  },
];

/** Is this the shortcut's key list that Settings shows on this OS? */
export const listedShortcuts = (platform: Platform = currentPlatform()): readonly Shortcut[] =>
  SHORTCUTS.filter((s) => !s.onlyOn || s.onlyOn === platform);

// ── Changed keys ──

export type Overrides = Record<string, StoredCombo>;
let custom: Overrides = {};

/** Use these changed keys (from settings; an id this version doesn't know is ignored). */
export function setCustomKeys(next: Overrides | null | undefined) {
  custom = next ?? {};
}

/** The shortcut's keys on this OS now: its first key (the one you can change) then the extras. */
export function effectiveCombos(
  s: Shortcut,
  platform: Platform = currentPlatform(),
  overrides: Overrides = custom,
): Combo[] {
  const base = s[platform];
  const o = overrides[s.id];
  if (!o?.key) return base;
  const menu = platform === "mac" && (base[0]?.menu === true || s.menuItem === true);
  return [{ ...o, ...(menu ? { menu: true } : {}) }, ...base.slice(1)];
}

const sameCombo = (a: Combo, b: Combo) =>
  a.key === b.key && !!a.mod === !!b.mod && !!a.shift === !!b.shift && !!a.alt === !!b.alt && !!a.ctrl === !!b.ctrl;

const isLetterKey = (key: string) => /^[a-z]$/.test(key);

/** A key that already types a shifted character (?), so Shift isn't part of its name. */
const shiftIsTyped = (c: Combo) =>
  !!c.shift && !c.mod && !c.ctrl && !c.alt && c.key.length === 1 && !isLetterKey(c.key) && !/[0-9]/.test(c.key);

const KEY_GLYPHS: Record<string, { mac: string; win: string }> = {
  Enter: { mac: "↩", win: "Enter" },
  ArrowUp: { mac: "↑", win: "↑" },
  ArrowDown: { mac: "↓", win: "↓" },
  ArrowLeft: { mac: "←", win: "←" },
  ArrowRight: { mac: "→", win: "→" },
  Escape: { mac: "⎋", win: "Esc" },
  Tab: { mac: "⇥", win: "Tab" },
  Backspace: { mac: "⌫", win: "Backspace" },
  Delete: { mac: "⌦", win: "Delete" },
  " ": { mac: "Space", win: "Space" },
};

/** How a key shows: ⇧⌘N on a Mac, Ctrl+Shift+N on Windows. A named key shows as its symbol or
 *  short name (↑, Esc), never as the raw `KeyboardEvent.key` ("ArrowUp"). */
export function formatCombo(c: Combo, platform: Platform = currentPlatform()): string {
  const named = KEY_GLYPHS[c.key]?.[platform];
  const key = named ?? (c.key.length === 1 ? c.key.toUpperCase() : c.key);
  const shift = c.shift && !shiftIsTyped(c);
  if (platform === "mac") return `${c.ctrl ? "⌃" : ""}${c.alt ? "⌥" : ""}${shift ? "⇧" : ""}${c.mod ? "⌘" : ""}${key}`;
  return [c.mod && "Ctrl", c.alt && "Alt", shift && "Shift", key].filter(Boolean).join("+");
}

const KEY_WORDS: Record<string, { mac?: string; win?: string; both?: string }> = {
  Enter: { mac: "Return", win: "Enter" },
  ArrowUp: { both: "Up Arrow" },
  ArrowDown: { both: "Down Arrow" },
  ArrowLeft: { both: "Left Arrow" },
  ArrowRight: { both: "Right Arrow" },
  Escape: { both: "Escape" },
  Tab: { both: "Tab" },
  Backspace: { mac: "Delete", win: "Backspace" },
  Delete: { mac: "Forward Delete", win: "Delete" },
  " ": { both: "Space" },
  ",": { both: "Comma" },
  ".": { both: "Period" },
  "/": { both: "Slash" },
  ";": { both: "Semicolon" },
  "'": { both: "Apostrophe" },
  "[": { both: "Left Bracket" },
  "]": { both: "Right Bracket" },
  "\\": { both: "Backslash" },
  "-": { both: "Minus" },
  "=": { both: "Equals" },
  "`": { both: "Grave Accent" },
  "?": { both: "Question Mark" },
};

/** The same key in words, for screen readers: "Command Shift N", "Control Shift N" (Windows),
 *  "Up Arrow", "Space". The symbols (⇧⌘N) are not read out reliably. */
export function describeCombo(c: Combo, platform: Platform = currentPlatform()): string {
  const w = KEY_WORDS[c.key];
  const key = w?.[platform] ?? w?.both ?? (c.key.length === 1 ? c.key.toUpperCase() : c.key);
  const shift = c.shift && !shiftIsTyped(c);
  const parts =
    platform === "mac"
      ? [c.ctrl && "Control", c.alt && "Option", shift && "Shift", c.mod && "Command", key]
      : [c.mod && "Control", c.alt && "Alt", shift && "Shift", key];
  return parts.filter(Boolean).join(" ");
}

/** A key that would type something (no ⌘ / Ctrl / ⌥, not an F key), like ?: it must not run
 *  while you're typing in a text box. */
export const typesText = (c: Combo) => !c.mod && !c.ctrl && !c.alt && !/^F\d+$/.test(c.key);

const isFKey = (key: string) => /^F([1-9]|1[0-2])$/.test(key);
const isLetter = (key: string) => /^[a-z]$/.test(key);

/** Keys the OS owns, by modifiers + key (c = ⌃, m = ⌘ / Ctrl, a = ⌥, s = ⇧). Each says why. */
const OS_OWNED: Record<Platform, Record<string, string>> = {
  mac: {
    "m+q": "⌘Q quits an app on every Mac",
    "m+h": "⌘H hides the app on every Mac",
    "ma+h": "⌥⌘H hides all other apps on every Mac",
    "m+m": "⌘M minimises the window on every Mac",
    "m+`": "⌘` switches between an app's windows",
    "cm+f": "⌃⌘F is full screen on every Mac",
    "cm+q": "⌃⌘Q locks the screen",
    "ms+q": "⇧⌘Q logs you out",
    "ma+d": "⌥⌘D shows or hides the Dock",
    "cm+d": "⌃⌘D looks a word up",
    "c+F2": "⌃F2 moves to the menu bar",
    "c+F3": "⌃F3 moves to the Dock",
    "m+0": "⌘0 brings the Nest window back (Window menu)",
    "m+c": "⌘C copies in a text box",
    "m+v": "⌘V pastes in a text box",
    "m+x": "⌘X cuts in a text box",
    "m+a": "⌘A selects all in a text box",
  },
  win: {
    "m+c": "Ctrl+C copies in a text box",
    "m+v": "Ctrl+V pastes in a text box",
    "m+x": "Ctrl+X cuts in a text box",
    "m+a": "Ctrl+A selects all in a text box",
    "ms+v": "Ctrl+Shift+V pastes without formatting in a text box",
  },
};
// Quit keeps ⌘Q: it is the Mac's own key for it.
const OWNED_BY_SHORTCUT: Record<string, string> = { "mac:m+q": "quit" };

const sig = (c: Combo) => `${c.ctrl ? "c" : ""}${c.mod ? "m" : ""}${c.alt ? "a" : ""}${c.shift ? "s" : ""}+${c.key}`;

/** Why this key can't be used for shortcut `id`, in plain words, or null when it can. The same
 *  rules as shortcut_keys.rs (which refuses unsafe keys when saving), plus keys the OS owns. */
export function checkCombo(id: ShortcutId, c: Combo, platform: Platform = currentPlatform()): string | null {
  const named = c.key === "Enter" || isFKey(c.key);
  const altName = platform === "mac" ? "Option" : "Alt";
  const plainKey = c.key.length === 1 && (isLetter(c.key) || /[0-9]/.test(c.key) || ",./;'[]\\-=`".includes(c.key));
  if (!named && !plainKey) {
    return (c.shift || c.alt) && c.key.length === 1
      ? `Shift and ${altName} only go with letters, Enter and F keys. Try other keys.`
      : "Use a letter, a number, a punctuation key, Enter or an F key.";
  }
  if ((c.shift || c.alt) && !(isLetter(c.key) || named)) {
    return `Shift and ${altName} only go with letters, Enter and F keys. Try other keys.`;
  }
  if (platform === "win" && c.alt) {
    return "Alt keys belong to Windows, and some keyboards type letters with them. Use Ctrl, or Ctrl+Shift.";
  }
  if (!c.mod && !(platform === "mac" && c.ctrl) && !isFKey(c.key)) {
    return platform === "mac"
      ? "Hold ⌘ (or ⌃) as well, so it doesn't type a letter."
      : "Hold Ctrl as well, so it doesn't type a letter.";
  }
  const owned = OS_OWNED[platform][sig(c)];
  if (owned && OWNED_BY_SHORTCUT[`${platform}:${sig(c)}`] !== id) return `${owned}. Try other keys.`;
  return null;
}

/** What a key press means for the recorder: a combination, a message, or nothing yet (a modifier
 *  key on its own: the rest of the combination is still to come). */
export type Recorded = { combo: Combo } | { error: string } | null;

export function recordedCombo(e: KeyLike, platform: Platform = currentPlatform()): Recorded {
  if (["Meta", "Control", "Shift", "Alt", "AltGraph", "CapsLock", "OS", "Fn"].includes(e.key)) return null;
  if (platform === "win" && altGr(e))
    return {
      error: "AltGr types characters on this keyboard. Use Ctrl, or Ctrl+Shift.",
    };
  if (platform === "win" && e.metaKey)
    return {
      error: "Windows key shortcuts belong to Windows. Use Ctrl, or Ctrl+Shift.",
    };
  const combo: Combo = { key: keyOf(e) };
  if (platform === "mac" ? e.metaKey : e.ctrlKey) combo.mod = true;
  if (platform === "mac" && e.ctrlKey) combo.ctrl = true;
  if (e.shiftKey) combo.shift = true;
  if (e.altKey) combo.alt = true;
  return { combo };
}

/** The outcome of giving shortcut `id` a new key. */
export type Change =
  | { kind: "error"; message: string }
  /** It already has those keys. */
  | { kind: "same" }
  | { kind: "save"; overrides: Overrides }
  /** Another shortcut has those keys: ask, then save `overrides` (both change places). */
  | { kind: "swap"; with: Shortcut; overrides: Overrides; message: string };

const stored = (c: Combo): StoredCombo => ({
  key: c.key,
  ...(c.mod ? { mod: true } : {}),
  ...(c.shift ? { shift: true } : {}),
  ...(c.alt ? { alt: true } : {}),
  ...(c.ctrl ? { ctrl: true } : {}),
});

/** `overrides` with shortcut `s` set to `c` (or back to its default when `c` is its default key). */
function withKey(overrides: Overrides, s: Shortcut, c: Combo, platform: Platform): Overrides {
  const next = { ...overrides };
  const base = s[platform][0];
  if (base && sameCombo(base, c)) delete next[s.id];
  else next[s.id] = stored(c);
  return next;
}

/** Work out what giving shortcut `id` the key `combo` does. Pure: the Shortcuts tab asks, shows
 *  the message or the swap question, and only then saves. Two shortcuts never share a key. */
export function planChange(id: ShortcutId, combo: Combo, platform: Platform, overrides: Overrides): Change {
  const s = SHORTCUTS.find((x) => x.id === id);
  if (!s) return { kind: "error", message: "That isn't a shortcut Nest knows." };
  const bad = checkCombo(id, combo, platform);
  if (bad) return { kind: "error", message: bad };
  const cur = effectiveCombos(s, platform, overrides)[0];
  if (cur && sameCombo(cur, combo)) return { kind: "same" };
  const others = listedShortcuts(platform).filter((x) => x.id !== id);
  const holder = (c: Combo, skip: ShortcutId[]) => {
    for (const o of others) {
      if (skip.includes(o.id)) continue;
      const at = effectiveCombos(o, platform, overrides).findIndex((x) => sameCombo(x, c));
      if (at >= 0) return { shortcut: o, at };
    }
    return null;
  };
  const hit = holder(combo, []);
  if (!hit) return { kind: "save", overrides: withKey(overrides, s, combo, platform) };
  const keys = formatCombo(combo, platform);
  if (hit.at > 0) {
    return {
      kind: "error",
      message: `${keys} is also used by ${hit.shortcut.label}, as an extra key. Try other keys.`,
    };
  }
  if (!cur) {
    return {
      kind: "error",
      message: `${keys} is used by ${hit.shortcut.label}, and ${s.label} has no keys to swap. Try other keys.`,
    };
  }
  const theirs = checkCombo(hit.shortcut.id, cur, platform);
  if (theirs) {
    return {
      kind: "error",
      message: `${keys} is used by ${hit.shortcut.label}, which can't take ${formatCombo(cur, platform)} in exchange. Try other keys.`,
    };
  }
  const third = holder(cur, [hit.shortcut.id]);
  if (third) {
    return {
      kind: "error",
      message: `${keys} is used by ${hit.shortcut.label}, which can't take ${formatCombo(cur, platform)} in exchange (${third.shortcut.label} has it). Try other keys.`,
    };
  }
  const swapped = withKey(withKey(overrides, s, combo, platform), hit.shortcut, cur, platform);
  return {
    kind: "swap",
    with: hit.shortcut,
    overrides: swapped,
    message: `Already used by ${hit.shortcut.label}. Swap them?`,
  };
}

/** Put shortcut `id` back to its default key: a change like any other (its default may be in use
 *  now, which asks to swap). Nothing to ask when it has no default key. */
export function planReset(id: ShortcutId, platform: Platform, overrides: Overrides): Change {
  const s = SHORTCUTS.find((x) => x.id === id);
  const base = s?.[platform][0];
  if (!s || !base) {
    const next = { ...overrides };
    delete next[id];
    return { kind: "save", overrides: next };
  }
  const without = { ...overrides };
  delete without[id];
  const change = planChange(id, base, platform, overrides);
  // Back to default means no entry at all.
  return change.kind === "same" ? { kind: "save", overrides: without } : change;
}

/** The keyboard event fields the matcher reads (a real KeyboardEvent has them all). */
export type KeyLike = Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"> & {
  getModifierState?: (key: string) => boolean;
};

export const currentPlatform = (): Platform => (isMac() ? "mac" : "win");

/** AltGr (Windows reports it as Ctrl+Alt): it types characters, so it's never a shortcut. */
const altGr = (e: KeyLike) => e.getModifierState?.("AltGraph") === true;

/** The key, lower case for letters. The character the key types wins when it is plain ASCII and
 *  ⌥ / Alt isn't held, so other layouts work (on a French keyboard `,` and `?` sit on the M key:
 *  they are "," and "?", not "m"). Where the character isn't a usable guide (⌥ turns a letter
 *  into å, ∂…; a dead key), the physical key (`code`) says which letter it is. */
function keyOf(e: KeyLike): string {
  const k = e.key;
  const plainAscii = k.length === 1 && k.charCodeAt(0) > 32 && k.charCodeAt(0) < 127;
  if (plainAscii && !e.altKey) return /[A-Za-z]/.test(k) ? k.toLowerCase() : k;
  const letter = /^Key([A-Z])$/.exec(e.code ?? "");
  if (letter && (e.altKey || k === "Dead" || (k.length === 1 && k.charCodeAt(0) > 127))) return letter[1].toLowerCase();
  return /^[a-z]$/i.test(k) ? k.toLowerCase() : k;
}

/** Does this key press match the combo exactly (no extra modifiers)? */
export function matchesCombo(e: KeyLike, c: Combo, platform: Platform = currentPlatform()): boolean {
  if (platform === "win" && altGr(e)) return false;
  const mod = platform === "mac" ? e.metaKey : e.ctrlKey;
  // The other OS's modifier must be up; on a Mac ⌃ counts only for combos that ask for it.
  const other = platform === "mac" ? e.ctrlKey !== !!c.ctrl : e.metaKey;
  if (other || mod !== !!c.mod || e.shiftKey !== !!c.shift || e.altKey !== !!c.alt) return false;
  return keyOf(e) === c.key;
}

export interface Hit {
  shortcut: Shortcut;
  combo: Combo;
}

/** The shortcut this key press is, among `ids` (default: all), or null. */
export function findShortcut(
  e: KeyLike,
  ids?: readonly ShortcutId[],
  platform: Platform = currentPlatform(),
): Hit | null {
  for (const shortcut of SHORTCUTS) {
    if (ids && !ids.includes(shortcut.id)) continue;
    const combo = effectiveCombos(shortcut, platform).find((c) => matchesCombo(e, c, platform));
    if (combo) return { shortcut, combo };
  }
  return null;
}

/** Is this key press the shortcut `id`? */
export const isShortcut = (e: KeyLike, id: ShortcutId, platform: Platform = currentPlatform()) =>
  findShortcut(e, [id], platform) !== null;

/** Somewhere you type, or a control that uses Escape itself (a dropdown). */
const ownsEscape = (t: EventTarget | null) =>
  t instanceof Element &&
  t.closest("input, textarea, select, [contenteditable]:not([contenteditable='false'])") !== null;

/** Windows: does this key close the Settings window? Ctrl+W always; Escape when you're not typing
 *  and nothing inside (a dropdown, a name being edited) used it. macOS: never (⌘W is the menu
 *  bar's Close Window there). */
export function closesSettings(
  e: KeyLike & Pick<KeyboardEvent, "target" | "defaultPrevented">,
  platform: Platform = currentPlatform(),
): boolean {
  if (platform !== "win") return false;
  if (isShortcut(e, "close-window", platform)) return true;
  return (
    e.key === "Escape" &&
    !e.defaultPrevented &&
    !e.ctrlKey &&
    !e.altKey &&
    !e.shiftKey &&
    !e.metaKey &&
    !ownsEscape(e.target)
  );
}

// ── The webview's own keys (Windows) ──

const BROWSER_LETTERS = new Set([
  "r", // reload (Shift: hard reload)
  "p", // print
  "f", // find bar (Nest's Ctrl+F focuses its own search instead)
  "g", // find next / previous
  "o", // open a file in the browser (Nest's Ctrl+O opens the project folder instead)
]);
const ZOOM_KEYS = new Set(["=", "+", "-", "_", "0"]);
const ZOOM_CODES = new Set(["NumpadAdd", "NumpadSubtract", "Numpad0"]);

/** Windows: is this one of WebView2's browser keys (reload, print, find, zoom, caret browsing)?
 *  They would reload the whole app or show browser UI, so every window blocks them. Editing keys
 *  (copy, paste, undo, select all, arrows…) are never in this list. */
export function isBrowserKey(e: KeyLike, platform: Platform = currentPlatform()): boolean {
  if (platform !== "win" || altGr(e) || e.metaKey) return false;
  const plain = !e.ctrlKey && !e.altKey;
  // F5 reload (Ctrl+F5 / Shift+F5 too), F3 find next, F7 caret browsing.
  if ((e.key === "F5" && !e.altKey) || (plain && (e.key === "F3" || e.key === "F7"))) return true;
  if (!e.ctrlKey || e.altKey) return false;
  if (BROWSER_LETTERS.has(keyOf(e))) return true;
  return ZOOM_KEYS.has(e.key) || ZOOM_CODES.has(e.code);
}

/** Windows: block the webview's own reload / print / find / zoom in this window (keys and
 *  Ctrl+wheel). Nest's own handlers still see the keys (Ctrl+R still refreshes the list). Runs
 *  first (capture), so no early return in a screen's handler can let a reload through. */
export function installBrowserKeyGuard(target: Window = window, platform: Platform = currentPlatform()): () => void {
  if (platform !== "win") return () => {};
  const onKey = (e: KeyboardEvent) => {
    if (isBrowserKey(e, platform)) e.preventDefault();
  };
  const onWheel = (e: WheelEvent) => {
    if (e.ctrlKey) e.preventDefault();
  };
  target.addEventListener("keydown", onKey, true);
  target.addEventListener("wheel", onWheel, { capture: true, passive: false });
  return () => {
    target.removeEventListener("keydown", onKey, true);
    target.removeEventListener("wheel", onWheel, true);
  };
}

// ── Menu bar commands (macOS) ──

/** Listen for an event sent to this window only (Rust's `emit_to`). A plain `listen` hears events
 *  sent to every window, which is how ⌘N once reached the Settings window too. */
export function listenHere<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  let label: unknown;
  try {
    label = getCurrentWindow().label;
  } catch {
    label = undefined; // outside Tauri (tests)
  }
  return typeof label === "string"
    ? listen<T>(event, handler, { target: { kind: "WebviewWindow", label } })
    : listen<T>(event, handler);
}

// ── Loading the changed keys, and the Show Keyboard Shortcuts key ──

/** Use the saved changed keys in this window now, and again whenever settings change. */
export function installCustomKeys(): () => void {
  const load = () =>
    getSettings()
      .then((v) => setCustomKeys(v.settings.shortcuts))
      .catch(() => {});
  void load();
  let unlisten: UnlistenFn | undefined;
  let stopped = false;
  listen("settings-changed", () => void load())
    .then((u) => (stopped ? u() : (unlisten = u)))
    .catch(() => {});
  return () => {
    stopped = true;
    unlisten?.();
  };
}

const isTypingTarget = (t: EventTarget | null) =>
  t instanceof Element &&
  t.closest("input, textarea, select, [contenteditable]:not([contenteditable='false'])") !== null;

/** Windows: F1 (and ? when you're not typing) opens Settings → Shortcuts, from any window. On a
 *  Mac the Help menu item has the key (menu.rs), so the page only keeps the key from doing
 *  anything else. */
export function installHelpKey(target: Window = window, platform: Platform = currentPlatform()): () => void {
  const onKey = (e: KeyboardEvent) => {
    const hit = findShortcut(e, ["keyboard-shortcuts"], platform);
    if (!hit || e.repeat) return;
    if (typesText(hit.combo) && isTypingTarget(e.target)) return;
    // macOS: the Help menu item owns the key; preventDefault would stop it reaching the menu.
    if (platform === "mac" && hit.combo.menu) return;
    e.preventDefault();
    void openSettings("shortcuts").catch(() => {});
  };
  target.addEventListener("keydown", onKey);
  return () => target.removeEventListener("keydown", onKey);
}
