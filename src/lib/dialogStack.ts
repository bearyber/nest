// The open modal dialogs in this window, bottom to top (useDialogFocus adds each one). Escape
// and a dialog's other window-wide keys only act in the top one, so with New Project open over
// the template editor, Escape closes New Project and leaves the editor alone.

const stack: HTMLElement[] = [];

/** Add an open dialog; returns its removal. A dialog inside another one goes above it, even if it
 *  was added first (React opens children before parents). */
export function pushDialog(el: HTMLElement): () => void {
  const inside = stack.findIndex((d) => el.contains(d) && d !== el);
  if (inside >= 0) stack.splice(inside, 0, el);
  else stack.push(el);
  return () => {
    const i = stack.lastIndexOf(el);
    if (i >= 0) stack.splice(i, 1);
  };
}

/** The top dialog, or null. */
export const topDialog = (): HTMLElement | null => stack[stack.length - 1] ?? null;

/** Is any modal dialog open in this window? */
export const hasOpenDialog = () => stack.length > 0;

/** Should this dialog's window-wide keys (Escape…) act? Only when it's the top one. A dialog that
 *  isn't on the stack (not drawn yet) acts only when no other dialog is open. */
export const isTopDialog = (el: HTMLElement | null) => (el && stack.includes(el) ? topDialog() === el : stack.length === 0);
