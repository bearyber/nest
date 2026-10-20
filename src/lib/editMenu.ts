// Edit → Undo / Redo from the macOS menu bar (Windows has no menu bar: Ctrl+Z / Ctrl+Y are
// handled by each screen's keys). The menu item is our own (src-tauri/src/menu.rs), so the
// predefined text-only Undo doesn't swallow it: in a text box it undoes the typing, anywhere
// else it's handed to whatever screen has its own history (the template editor).
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listenHere } from "./shortcuts";

export type EditStep = "undo" | "redo";

/** The window event a screen with its own Undo listens for (`detail` is the step). */
export const EDIT_STEP_EVENT = "nest-edit-step";

const TEXT_INPUTS = new Set(["", "text", "search", "url", "email", "tel", "password", "number"]);

/** Is the focus somewhere you type text? */
export function inTextBox(el: Element | null = document.activeElement): boolean {
  if (!el) return false;
  if (el.tagName === "TEXTAREA") return true;
  if (el.tagName === "INPUT") return TEXT_INPUTS.has((el.getAttribute("type") ?? "").toLowerCase());
  return (el as HTMLElement).isContentEditable === true;
}

/** Run one Undo / Redo in this window. */
export function runEditStep(step: EditStep) {
  if (inTextBox()) {
    // Still supported by WebKit and WebView2 for the text box's own undo.
    document.execCommand(step);
    return;
  }
  window.dispatchEvent(new CustomEvent<EditStep>(EDIT_STEP_EVENT, { detail: step }));
}

/** Listen for the menu's Undo / Redo. lib.rs sends them to the focused window only; the focus
 *  check here is a second guard. */
export function listenEditMenu(): () => void {
  let unlisten: (() => void) | undefined;
  let gone = false;
  listenHere<string>("menu", async (e) => {
    const step = e.payload;
    if (step !== "undo" && step !== "redo") return;
    let focused = document.hasFocus();
    try {
      focused = await getCurrentWindow().isFocused();
    } catch {
      // Outside Tauri: the page's own focus will do.
    }
    if (focused) runEditStep(step);
  })
    .then((u) => (gone ? u() : (unlisten = u)))
    .catch(() => {});
  return () => {
    gone = true;
    unlisten?.();
  };
}
