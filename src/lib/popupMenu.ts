// Right-click and pop-up menus. Each item is made on its own (MenuItem.new …) and kept until the
// next menu opens. Tauri 2.12 drops the click handler of an item written inline in
// Menu.new({ items }) as soon as the menu is built, so those items did nothing when clicked
// (found in 0.7.2: Open, Mark done, Copy job code…). Items get no ids: Tauri makes each one
// unique, so none can be mistaken for a menu bar item ("copy-path").
import { Menu, MenuItem, PredefinedMenuItem, Submenu } from "@tauri-apps/api/menu";

export type PopupItem =
  | { text: string; action?: () => void; enabled?: boolean }
  | { item: "Separator" }
  | { text: string; items: PopupItem[]; enabled?: boolean };

type Made = { close(): Promise<void> };
type Built = MenuItem | PredefinedMenuItem | Submenu;

/** What the last menu made: closed when the next one opens (a click can arrive just after
 *  `popup()` returns, so they can't be closed straight away). */
let kept: Made[] = [];

async function make(i: PopupItem, made: Made[]): Promise<Built> {
  let built: Built;
  if ("item" in i) {
    built = await PredefinedMenuItem.new({ item: "Separator" });
  } else if ("items" in i) {
    const kids: Built[] = [];
    for (const k of i.items) kids.push(await make(k, made));
    built = await Submenu.new({ text: i.text, enabled: i.enabled, items: kids });
  } else {
    const run = i.action;
    built = await MenuItem.new({ text: i.text, enabled: i.enabled, action: run ? () => run() : undefined });
  }
  made.push(built);
  return built;
}

/** Show a menu at the mouse. */
export async function popupMenu(items: PopupItem[]): Promise<void> {
  const previous = kept;
  kept = [];
  for (const r of previous) void r.close().catch(() => {});
  const made: Made[] = [];
  const top: Built[] = [];
  for (const i of items) top.push(await make(i, made));
  const menu = await Menu.new({ items: top });
  made.push(menu);
  kept = made;
  await menu.popup();
}
