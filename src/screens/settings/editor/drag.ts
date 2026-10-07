// Drag to move, built on pointer events. HTML5 drag-and-drop is swallowed by Tauri's file-drop
// handler in the Windows webview; pointer events behave the same on both OSes.
import { useRef, useState } from "react";
import type { DropWhere } from "../../../lib/templateDraft";

export interface DropTarget {
  id: string;
  where: DropWhere;
}

interface Options {
  /** The data attribute that marks a drop target, e.g. `data-folder-id`. */
  attr: string;
  /** May `id` be dropped on `target`? (Not on itself or inside its own children.) */
  canDrop: (id: string, target: string) => boolean;
  /** May things go inside `target`? Otherwise only before/after. */
  canHoldInside?: (target: string) => boolean;
  onDrop: (id: string, target: DropTarget) => void;
}

const THRESHOLD = 4;

export function usePointerDrag({ attr, canDrop, canHoldInside, onDrop }: Options) {
  const [dragging, setDragging] = useState<string | null>(null);
  const [target, setTarget] = useState<DropTarget | null>(null);
  const start = useRef<{ id: string; x: number; y: number; moved: boolean } | null>(null);
  const targetRef = useRef<DropTarget | null>(null);

  const pick = (x: number, y: number, id: string): DropTarget | null => {
    const el = document.elementFromPoint(x, y)?.closest(`[${attr}]`) as HTMLElement | null;
    const over = el?.getAttribute(attr);
    if (!el || !over || !canDrop(id, over)) return null;
    const r = el.getBoundingClientRect();
    const f = (y - r.top) / r.height;
    const inside = canHoldInside?.(over) ?? false;
    const where: DropWhere = inside ? (f < 0.3 ? "before" : f > 0.7 ? "after" : "inside") : f < 0.5 ? "before" : "after";
    return { id: over, where };
  };

  const handlers = (id: string) => ({
    onPointerDown: (e: React.PointerEvent) => {
      if (e.button !== 0 || (e.target as HTMLElement).closest("input, select, button, textarea")) return;
      start.current = { id, x: e.clientX, y: e.clientY, moved: false };
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    },
    onPointerMove: (e: React.PointerEvent) => {
      const s = start.current;
      if (!s) return;
      if (!s.moved && Math.hypot(e.clientX - s.x, e.clientY - s.y) < THRESHOLD) return;
      if (!s.moved) {
        s.moved = true;
        setDragging(s.id);
      }
      const t = pick(e.clientX, e.clientY, s.id);
      targetRef.current = t;
      setTarget(t);
    },
    onPointerUp: () => {
      const s = start.current;
      start.current = null;
      if (s?.moved && targetRef.current) onDrop(s.id, targetRef.current);
      targetRef.current = null;
      setDragging(null);
      setTarget(null);
    },
    onPointerCancel: () => {
      start.current = null;
      targetRef.current = null;
      setDragging(null);
      setTarget(null);
    },
  });

  return { dragging, target, handlers };
}
