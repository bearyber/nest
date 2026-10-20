// Nest's keyboard shortcuts, in one place (build spec §8). Each has an id, a label, and its keys
// on macOS and on Windows. On macOS the main ones live in the menu bar (src-tauri/src/menu.rs):
// those keys are marked `menu: true`, and menu.rs must use the same accelerators
// (shortcuts.test.ts checks it). Windows has no menu bar, so the page's keydown handlers run them.
//
// Matching is exact: Ctrl+Shift+R is not Ctrl+R, ⌃⌘F (full screen) is not ⌘F, and AltGr (which
// Windows reports as Ctrl+Alt) never counts as a shortcut, since it types characters.
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isMac, type Platform } from "./platform";

export type ShortcutId =
  | "new-project"
  | "open-settings"
  | "rescan"
  | "search"
  | "toggle-sidebar"
  | "toggle-inspector"
  | "undo"
  | "redo"
  | "create"
  | "close-window";

/** One key combination. */
export interface Combo {
  /** `KeyboardEvent.key`: a lower-case letter ("n"), a character (","), or a key name ("F5"). */
  key: string;
  /** ⌘ on macOS, Ctrl on Windows. */
  mod?: boolean;
  shift?: boolean;
  /** ⌥ on macOS, Alt on Windows. */
  alt?: boolean;
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
    mac: [{ key: "f", mod: true, menu: true }, { key: "k", mod: true }],
    win: [{ key: "f", mod: true }, { key: "k", mod: true }],
  },
  {
    id: "toggle-sidebar",
    label: "Toggle Sidebar",
    mac: [{ key: "b", mod: true, menu: true }],
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
    id: "undo",
    label: "Undo",
    mac: [{ key: "z", mod: true, menu: true }],
    win: [{ key: "z", mod: true }],
  },
  {
    id: "redo",
    label: "Redo",
    mac: [{ key: "z", mod: true, shift: true, menu: true }],
    win: [{ key: "y", mod: true }, { key: "z", mod: true, shift: true }],
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
    // macOS: the menu bar's own Close Window item (predefined in menu.rs).
    mac: [{ key: "w", mod: true, menu: true }],
    win: [{ key: "w", mod: true }],
  },
];

/** The keyboard event fields the matcher reads (a real KeyboardEvent has them all). */
export type KeyLike = Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"> & {
  getModifierState?: (key: string) => boolean;
};

export const currentPlatform = (): Platform => (isMac() ? "mac" : "win");

/** AltGr (Windows reports it as Ctrl+Alt): it types characters, so it's never a shortcut. */
const altGr = (e: KeyLike) => e.getModifierState?.("AltGraph") === true;

/** The key, lower case for letters. ⌥ on a Mac (and other keyboard layouts) can turn a letter
 *  into another character: then the physical key (`code`) says which letter it is. */
function keyOf(e: KeyLike): string {
  if (/^[a-z]$/i.test(e.key)) return e.key.toLowerCase();
  if (e.key.length === 1 && /^Key[A-Z]$/.test(e.code)) return e.code.slice(3).toLowerCase();
  return e.key;
}

/** Does this key press match the combo exactly (no extra modifiers)? */
export function matchesCombo(e: KeyLike, c: Combo, platform: Platform = currentPlatform()): boolean {
  if (platform === "win" && altGr(e)) return false;
  const mod = platform === "mac" ? e.metaKey : e.ctrlKey;
  const other = platform === "mac" ? e.ctrlKey : e.metaKey;
  if (other || mod !== !!c.mod || e.shiftKey !== !!c.shift || e.altKey !== !!c.alt) return false;
  return keyOf(e) === c.key;
}

export interface Hit {
  shortcut: Shortcut;
  combo: Combo;
}

/** The shortcut this key press is, among `ids` (default: all), or null. */
export function findShortcut(e: KeyLike, ids?: readonly ShortcutId[], platform: Platform = currentPlatform()): Hit | null {
  for (const shortcut of SHORTCUTS) {
    if (ids && !ids.includes(shortcut.id)) continue;
    const combo = shortcut[platform].find((c) => matchesCombo(e, c, platform));
    if (combo) return { shortcut, combo };
  }
  return null;
}

/** Is this key press the shortcut `id`? */
export const isShortcut = (e: KeyLike, id: ShortcutId, platform: Platform = currentPlatform()) =>
  findShortcut(e, [id], platform) !== null;

/** Somewhere you type, or a control that uses Escape itself (a dropdown). */
const ownsEscape = (t: EventTarget | null) =>
  t instanceof Element && t.closest("input, textarea, select, [contenteditable]:not([contenteditable='false'])") !== null;

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
