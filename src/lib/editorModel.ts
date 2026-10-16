// Pure helpers for the one-screen template editor (PLAN 2026-10-13): names as text and blank
// chips, outline moves (add, indent, outdent, delete) and undo history. No UI, no Tauri.
import { cloneNodes, locate, nodeId, type FolderNode } from "./templateDraft";
import type { FileEntry } from "./types";

// ───────────────────────── Names: text and blanks ─────────────────────────

/** A piece of a name: plain text, or a blank (`{key}` / `{key|transform}`). */
export type NamePiece = { text: string } | { key: string; transform?: string };

export const isBlank = (p: NamePiece): p is { key: string; transform?: string } => "key" in p;

/** `{start|yymmdd}_{name|caps}` → [blank, "_", blank]. A `{` without a matching `}` stays text,
 *  so nothing typed is ever lost (Rust says what's wrong with it). */
export function parseName(s: string): NamePiece[] {
  const out: NamePiece[] = [];
  const re = /\{([A-Za-z0-9_]+)(?:\|([a-z]+))?\}/g;
  let last = 0;
  for (let m = re.exec(s); m; m = re.exec(s)) {
    if (m.index > last) out.push({ text: s.slice(last, m.index) });
    out.push(m[2] ? { key: m[1], transform: m[2] } : { key: m[1] });
    last = m.index + m[0].length;
  }
  if (last < s.length) out.push({ text: s.slice(last) });
  return out;
}

export const formatName = (pieces: NamePiece[]) =>
  pieces.map((p) => (isBlank(p) ? `{${p.key}${p.transform ? `|${p.transform}` : ""}}` : p.text)).join("");

const blankText = (key: string, transform?: string) => `{${key}${transform ? `|${transform}` : ""}}`;

/** Put a blank into `name` at a text position (e.g. where the cursor is). */
export const insertBlank = (name: string, at: number, key: string, transform?: string) =>
  name.slice(0, at) + blankText(key, transform) + name.slice(at);

/** "Make a blank": replace the selected part of `name` (`start`–`end`) with a blank. A selection
 *  that cuts into an existing blank is widened to cover it, so no half blank is left. */
export function makeBlank(name: string, start: number, end: number, key: string, transform?: string): string {
  let [a, b] = start <= end ? [start, end] : [end, start];
  const re = /\{[^{}]*\}/g;
  for (let m = re.exec(name); m; m = re.exec(name)) {
    const s = m.index;
    const e = s + m[0].length;
    if (s < b && e > a) {
      a = Math.min(a, s);
      b = Math.max(b, e);
    }
  }
  return name.slice(0, a) + blankText(key, transform) + name.slice(b);
}

/** The blank keys a name uses, in order, once each. */
export function blanksIn(name: string): string[] {
  const keys = parseName(name).flatMap((p) => (isBlank(p) ? [p.key] : []));
  return [...new Set(keys)];
}

// ───────────────────────── Outline moves ─────────────────────────

const newFolder = (name: string): FolderNode => ({ id: nodeId(), name, explicit: true, kids: [] });

/** Add a folder inside `parentId` (null = the project folder), after `afterId` if given, else
 *  at the end. "One per" folders hold no fixed folders, so nothing is added there. */
export function addFolder(
  nodes: FolderNode[],
  parentId: string | null,
  name: string,
  afterId?: string,
): { nodes: FolderNode[]; id: string | null } {
  const next = cloneNodes(nodes);
  let list = next;
  if (parentId) {
    const at = locate(next, parentId);
    if (!at || at.node.each) return { nodes, id: null };
    list = at.node.kids;
  }
  const folder = newFolder(name);
  const after = afterId ? list.findIndex((n) => n.id === afterId) : -1;
  if (after >= 0) list.splice(after + 1, 0, folder);
  else list.push(folder);
  return { nodes: next, id: folder.id };
}

/** The folder a folder is in (null = the project folder). */
export function parentOf(nodes: FolderNode[], id: string): string | null {
  const go = (list: FolderNode[], parent: string | null): string | null | undefined => {
    for (const n of list) {
      if (n.id === id) return parent;
      const r = go(n.kids, n.id);
      if (r !== undefined) return r;
    }
    return undefined;
  };
  return go(nodes, null) ?? null;
}

/** Tab: move a folder into the folder just above it (as its last folder). `null` when there's
 *  no folder above, or it's a "one per" folder. */
export function indent(nodes: FolderNode[], id: string): FolderNode[] | null {
  const next = cloneNodes(nodes);
  const at = locate(next, id);
  if (!at || at.index === 0) return null;
  const above = at.list[at.index - 1];
  if (above.each) return null;
  at.list.splice(at.index, 1);
  above.kids.push(at.node);
  return next;
}

/** Shift+Tab: move a folder out of its folder, to just after it. `null` at the top level. */
export function outdent(nodes: FolderNode[], id: string): FolderNode[] | null {
  const parentId = parentOf(nodes, id);
  if (!parentId) return null;
  const next = cloneNodes(nodes);
  const at = locate(next, id);
  const parent = locate(next, parentId);
  if (!at || !parent) return null;
  at.list.splice(at.index, 1);
  parent.list.splice(parent.index + 1, 0, at.node);
  return next;
}

/** Delete a folder and everything in it (Undo brings it back). */
export function removeFolder(nodes: FolderNode[], id: string): FolderNode[] {
  const next = cloneNodes(nodes);
  const at = locate(next, id);
  if (at) at.list.splice(at.index, 1);
  return next;
}

/** Files that live in `path` (straight inside, not in its folders). */
export const filesIn = (files: FileEntry[] = [], path: string) =>
  files.filter((f) => {
    const slash = f.to.lastIndexOf("/");
    return (slash < 0 ? "" : f.to.slice(0, slash)) === path;
  });

/** Files gone with a deleted folder: the ones inside it, at any depth. */
export const withoutFilesIn = (files: FileEntry[] = [], path: string) =>
  files.filter((f) => !f.to.startsWith(`${path}/`));

// ───────────────────────── Undo ─────────────────────────

export interface History<T> {
  past: T[];
  present: T;
  future: T[];
}

/** Steps kept for Undo. */
export const UNDO_STEPS = 100;

export const startHistory = <T>(present: T): History<T> => ({ past: [], present, future: [] });

/** A new step: the old present can be undone to; redo is cleared. */
export function record<T>(h: History<T>, next: T): History<T> {
  if (Object.is(next, h.present)) return h;
  return { past: [...h.past, h.present].slice(-UNDO_STEPS), present: next, future: [] };
}

export function undo<T>(h: History<T>): History<T> {
  if (h.past.length === 0) return h;
  return { past: h.past.slice(0, -1), present: h.past[h.past.length - 1], future: [h.present, ...h.future] };
}

export function redo<T>(h: History<T>): History<T> {
  if (h.future.length === 0) return h;
  return { past: [...h.past, h.present], present: h.future[0], future: h.future.slice(1) };
}

// ───────────────────────── Question order ─────────────────────────

/** Move question `key` to just before / after `target`: the order New Project asks them in.
 *  The client question stays first (it can't move, and nothing goes before it). The same array
 *  back means nothing changed. */
export function moveField<F extends { key: string; type: string }>(
  fields: F[],
  key: string,
  target: string,
  where: "before" | "after",
): F[] {
  const moving = fields.find((f) => f.key === key);
  if (!moving || moving.type === "client" || key === target) return fields;
  const rest = fields.filter((f) => f.key !== key);
  let at = rest.findIndex((f) => f.key === target);
  if (at < 0) return fields;
  if (where === "after") at += 1;
  let firstFree = 0;
  while (firstFree < rest.length && rest[firstFree].type === "client") firstFree += 1;
  at = Math.max(at, firstFree);
  const next = [...rest.slice(0, at), moving, ...rest.slice(at)];
  return next.every((f, i) => f === fields[i]) ? fields : next;
}
