import { describe, expect, it, vi } from "vitest";

const { made, fake } = vi.hoisted(() => {
  const made: { kind: string; opts: Record<string, unknown>; closed: boolean }[] = [];
  const fake = (kind: string) => async (opts: Record<string, unknown>) => {
    const r = { kind, opts, closed: false, close: async () => void (r.closed = true), popup: async () => {} };
    made.push(r);
    return r;
  };
  return { made, fake };
});
vi.mock("@tauri-apps/api/menu", () => ({
  Menu: { new: fake("menu") },
  MenuItem: { new: fake("item") },
  PredefinedMenuItem: { new: fake("separator") },
  Submenu: { new: fake("submenu") },
}));

import { popupMenu } from "./popupMenu";

describe("pop-up menus", () => {
  it("makes every item on its own, with its action and no id (Tauri 2.12 drops inline ones' clicks)", async () => {
    const open = vi.fn();
    await popupMenu([
      { text: "Open", action: open },
      { item: "Separator" },
      { text: "More", items: [{ text: "Inside", action: () => {} }] },
    ]);
    expect(made.map((m) => m.kind)).toEqual(["item", "separator", "item", "submenu", "menu"]);
    const item = made[0].opts as { action?: () => void; id?: string };
    expect(item.id).toBeUndefined();
    item.action?.();
    expect(open).toHaveBeenCalled();
  });

  it("keeps a menu's items until the next menu opens, then lets them go", async () => {
    const before = made.length;
    await popupMenu([{ text: "Again" }]);
    expect(made.slice(0, before).every((m) => m.closed)).toBe(true);
    expect(made.slice(before).some((m) => m.closed)).toBe(false);
  });
});
