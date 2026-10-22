// Which area of the main window "has the selection" (macOS, like Finder: the selected row is
// accent blue only in the area you last used; the other area's selection is grey). Elements mark
// themselves `data-area="list"` / `"sidebar"`; this keeps `<html data-area>` on the last area you
// clicked in or moved focus into ("other" when it's neither, e.g. the Details panel). It is
// tracked by hand because WebKit doesn't focus a button or row when you click it, so
// `:focus-within` alone would never see a click.

/** The area an element is in, or "other". */
export function areaOf(target: EventTarget | null): string {
  return (target instanceof Element && target.closest("[data-area]")?.getAttribute("data-area")) || "other";
}

export function trackFocusArea(root: HTMLElement = document.documentElement, doc: Document = document): () => void {
  root.dataset.area ||= "list";
  const set = (e: Event) => {
    root.dataset.area = areaOf(e.target);
  };
  doc.addEventListener("pointerdown", set, true);
  doc.addEventListener("focusin", set, true);
  return () => {
    doc.removeEventListener("pointerdown", set, true);
    doc.removeEventListener("focusin", set, true);
  };
}
