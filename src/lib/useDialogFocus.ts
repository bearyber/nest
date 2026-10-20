import { useLayoutEffect, useRef, type RefObject } from "react";

/** Where focus goes when a dialog opens (unless something inside already took it, e.g. autoFocus).
 *  - "field": the element marked `data-autofocus`, else the first text box / dropdown, else the
 *    first button or link, else the dialog itself. Fields that appear a moment later (loaded
 *    content) still get focus, as long as nobody moved it in the meantime.
 *  - "container": the dialog itself (its own Escape / Enter keys keep working). */
export type InitialFocus = "field" | "container";

export interface DialogFocusOptions {
  /** Modal (default): Tab stays inside and the rest of the app is `inert` while it's open.
   *  Non-modal panels only get focus moved in on open and back on close. */
  modal?: boolean;
  initialFocus?: InitialFocus;
}

const FOCUSABLE = [
  "a[href]",
  "button",
  "input",
  "select",
  "textarea",
  "[contenteditable]:not([contenteditable='false'])",
  "[tabindex]",
].join(",");

const FIELD = "input:not([type='checkbox']):not([type='radio']):not([type='hidden']), textarea, select";

const usable = (el: HTMLElement) =>
  !(el as HTMLButtonElement).disabled && !el.closest("[inert], [hidden]") && !(el instanceof HTMLInputElement && el.type === "hidden");

/** Elements Tab can reach inside `root`, in document order. */
export function tabbables(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.tabIndex >= 0 && usable(el));
}

function initialTarget(root: HTMLElement, mode: InitialFocus): HTMLElement | null {
  if (mode === "container") return root;
  const marked = root.querySelector<HTMLElement>("[data-autofocus]");
  if (marked && usable(marked)) return marked;
  const field = Array.from(root.querySelectorAll<HTMLElement>(FIELD)).find(usable);
  return field ?? tabbables(root)[0] ?? null;
}

/** Sets `inert` on everything outside `el` (each ancestor's other children, up to <body>), also on
 *  siblings added while it's open. Returns the undo, which only clears what it set itself, so a
 *  dialog inside another dialog leaves the outer one's `inert` alone. */
function inertOutside(el: HTMLElement): () => void {
  const set: Element[] = [];
  const onPath = new Set<Element>();
  const mark = (sib: Element) => {
    if (onPath.has(sib) || sib.hasAttribute("inert") || sib.tagName === "SCRIPT" || sib.tagName === "STYLE") return;
    sib.setAttribute("inert", "");
    set.push(sib);
  };
  const parents: Element[] = [];
  for (let node: Element = el; node.parentElement && node !== document.body; node = node.parentElement) {
    onPath.add(node);
    parents.push(node.parentElement);
  }
  for (const p of parents) Array.from(p.children).forEach(mark);
  const watch =
    typeof MutationObserver === "undefined"
      ? null
      : new MutationObserver((records) => {
          // A modal dialog that opened on top (e.g. New Project from the menu bar while the template
          // editor is up) stays usable: it makes this one inert instead.
          const opensDialog = (n: Element) => n.matches("[data-modal-dialog]") || n.querySelector("[data-modal-dialog]");
          for (const r of records) r.addedNodes.forEach((n) => n instanceof Element && !opensDialog(n) && mark(n));
        });
  parents.forEach((p) => watch?.observe(p, { childList: true }));
  return () => {
    watch?.disconnect();
    for (const s of set) s.removeAttribute("inert");
  };
}

/** Keyboard and screen-reader basics for a dialog or panel (build spec §8, native feel):
 *  focus moves in when it opens, Tab / Shift+Tab stay inside (modal), the rest of the app is inert
 *  (modal), and focus goes back to where it was when it closes. Give the dialog `tabIndex={-1}`.
 *  Works for a dialog opened inside another dialog. */
export function useDialogFocus<T extends HTMLElement>(
  ref: RefObject<T | null>,
  { modal = true, initialFocus = "field" }: DialogFocusOptions = {},
) {
  // Options are read once, on open: the dialog's behaviour doesn't change while it's up.
  const opts = useRef({ modal, initialFocus });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { modal, initialFocus } = opts.current;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (!el.hasAttribute("tabindex")) el.tabIndex = -1;
    if (modal) el.dataset.modalDialog = "";
    const undoInert = modal ? inertOutside(el) : () => {};

    const focusStart = () => (initialTarget(el, initialFocus) ?? el).focus();
    if (!el.contains(document.activeElement)) focusStart();

    // Content that loads after opening (NewProject's fields): focus the first field once it shows,
    // unless the person already moved focus somewhere else.
    let late: MutationObserver | null = null;
    if (initialFocus === "field" && document.activeElement === el && typeof MutationObserver !== "undefined") {
      late = new MutationObserver(() => {
        if (document.activeElement !== el) {
          late?.disconnect();
          return;
        }
        const t = initialTarget(el, "field");
        if (t) {
          t.focus();
          late?.disconnect();
        }
      });
      late.observe(el, { childList: true, subtree: true });
    }

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Tab" || e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return;
      // A modal dialog inside this one handles its own Tab.
      const target = e.target instanceof Element ? e.target : null;
      if (target?.closest("[data-modal-dialog]") !== el) return;
      const items = tabbables(el);
      const active = document.activeElement as HTMLElement | null;
      if (items.length === 0) {
        e.preventDefault();
        el.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      const i = active ? items.indexOf(active) : -1;
      if (e.shiftKey && (active === first || active === el || i === -1)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || i === -1)) {
        e.preventDefault();
        first.focus();
      }
    };
    if (modal) el.addEventListener("keydown", onKeyDown);

    return () => {
      late?.disconnect();
      if (modal) {
        el.removeEventListener("keydown", onKeyDown);
        delete el.dataset.modalDialog;
      }
      undoInert();
      // Only take focus back if it's still ours (or lost); never pull it from where the person clicked.
      const active = document.activeElement;
      const ours = !active || active === document.body || el.contains(active);
      if (!ours) return;
      if (before && before !== el && before.isConnected && !before.closest("[inert]") && !el.contains(before)) {
        before.focus();
      } else {
        // The opener is gone (e.g. a row that was removed): the enclosing dialog, if any.
        el.parentElement?.closest<HTMLElement>("[role='dialog']")?.focus();
      }
    };
  }, [ref]);
}
