/** Things that own their Enter / Space / arrow keys: the window-wide list keys leave them alone. */
const OWNS_KEYS = [
  "input",
  "textarea",
  "select",
  "button",
  "a[href]",
  "[contenteditable]:not([contenteditable='false'])",
  ...[
    "button",
    "link",
    "tab",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "checkbox",
    "radio",
    "switch",
    "combobox",
    "listbox",
    "tree",
    "treeitem",
    "slider",
    "spinbutton",
    "dialog",
    "alertdialog",
  ].map((r) => `[role='${r}']`),
  "dialog",
].join(",");

/** Where a plain Enter / Space / arrow key means "the project list": nothing focused (body), the
 *  main area, or the list itself. */
const LIST_AREA = "main, [role='grid']";

/** Whether a plain Enter / Space / arrow key press is for the project list (open, inspector,
 *  move the selection). False when something already handled it or a focused control owns it:
 *  a button, link, text box, menu, tab, dialog… so Enter on "New" does New, not "open folder". */
export function isListKey(e: KeyboardEvent): boolean {
  if (e.defaultPrevented) return false;
  const t = e.target;
  if (!(t instanceof Element)) return true; // the window or document: nothing focused
  if (t === document.body || t === document.documentElement) return true;
  if (t.closest(OWNS_KEYS)) return false;
  return t.closest(LIST_AREA) !== null;
}
