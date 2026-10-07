// Pure helpers for the template editor: the folder tree, folder-name blocks and question keys.
// Rust validates and saves; these only turn the template format into something editable and back.
import type { Field, FieldKind, FileEntry, Plan, Template, TemplatePreview, TreeEntry } from "./types";

// ───────────────────────── Folders ─────────────────────────

/** One folder in the editor's tree. `explicit`: the template lists it as its own entry (a parent
 *  that only exists because of its children isn't written back, so nothing gets added). */
export interface FolderNode {
  id: string;
  name: string;
  /** Only made when this yes/no question is ticked (`!key`: when it isn't). */
  when?: string;
  /** One folder per picked option of this question; the name is `{item}`. */
  each?: string;
  explicit: boolean;
  kids: FolderNode[];
}

let nextId = 0;
export const nodeId = () => `n${++nextId}`;

const entryName = (e: TreeEntry) => (typeof e === "string" ? e : e.name);

/** Template tree entries → nested folders. */
export function toNodes(tree: TreeEntry[] = []): FolderNode[] {
  const root: FolderNode[] = [];
  for (const entry of tree) {
    const segs = entryName(entry).split("/");
    let level = root;
    segs.forEach((seg, i) => {
      const last = i === segs.length - 1;
      let node = level.find((n) => n.name === seg && !n.each);
      if (!node || (last && typeof entry !== "string" && entry.each)) {
        node = { id: nodeId(), name: seg, explicit: false, kids: [] };
        level.push(node);
      }
      if (last) {
        node.explicit = true;
        if (typeof entry !== "string") {
          if (entry.when) node.when = entry.when;
          if (entry.each) node.each = entry.each;
        }
      }
      level = node.kids;
    });
  }
  return root;
}

export interface TreeResult {
  tree: TreeEntry[];
  /** Plain-language reasons the tree can't be written (e.g. two different "only if" rules). */
  problems: string[];
}

/** Nested folders → template tree entries. A folder inside an "only if" or "one per" folder
 *  inherits that rule, because the format makes parent folders by itself (review fix 3). */
export function fromNodes(nodes: FolderNode[], labelOf: (key: string) => string = (k) => k): TreeResult {
  const tree: TreeEntry[] = [];
  const problems: string[] = [];
  const walk = (list: FolderNode[], prefix: string, when?: string, each?: string) => {
    for (const n of list) {
      const path = prefix + n.name;
      let w = when;
      if (n.when) {
        if (when && when !== n.when) {
          problems.push(
            `"${path}" is inside a folder that's only made if "${labelOf(strip(when))}" is ticked, so it can't have its own "only if". Move it out, or remove one of the rules.`,
          );
        }
        w = n.when;
      }
      let e = each;
      if (n.each) {
        if (each && each !== n.each) problems.push(`"${path}" can't be "one per" two different questions.`);
        e = n.each;
      }
      const write = n.kids.length === 0 || n.explicit;
      if (write) tree.push(w || e ? { name: path, ...(w ? { when: w } : {}), ...(e ? { each: e } : {}) } : path);
      walk(n.kids, `${path}/`, w, e);
    }
  };
  walk(nodes, "");
  return { tree, problems };
}

const strip = (when: string) => (when.startsWith("!") ? when.slice(1) : when);

/** Find a folder and the list it's in. */
export function locate(
  nodes: FolderNode[],
  id: string,
): { node: FolderNode; list: FolderNode[]; index: number; path: string } | null {
  const go = (list: FolderNode[], prefix: string): ReturnType<typeof locate> => {
    for (let i = 0; i < list.length; i++) {
      const n = list[i];
      const path = prefix + n.name;
      if (n.id === id) return { node: n, list, index: i, path };
      const r = go(n.kids, `${path}/`);
      if (r) return r;
    }
    return null;
  };
  return go(nodes, "");
}

/** Deep copy so React sees a new tree. */
export const cloneNodes = (nodes: FolderNode[]): FolderNode[] =>
  nodes.map((n) => ({ ...n, kids: cloneNodes(n.kids) }));

export type DropWhere = "before" | "after" | "inside";

/** Move a folder before, after or into another one. A folder can't go inside itself. */
export function moveNode(nodes: FolderNode[], id: string, targetId: string, where: DropWhere): FolderNode[] {
  if (id === targetId) return nodes;
  const next = cloneNodes(nodes);
  const from = locate(next, id);
  if (!from || locate(from.node.kids, targetId)) return nodes;
  from.list.splice(from.index, 1);
  const to = locate(next, targetId);
  if (!to) return nodes;
  if (where === "inside") {
    if (to.node.each) return nodes; // "one per" folders hold no fixed folders of their own here
    to.node.kids.push(from.node);
  } else {
    to.list.splice(where === "before" ? to.index : to.index + 1, 0, from.node);
  }
  return next;
}

/** Keep starter files pointing at a renamed or moved folder (review fix 4). */
export function movePathInFiles(files: FileEntry[] = [], oldPath: string, newPath: string): FileEntry[] {
  if (oldPath === newPath) return files;
  return files.map((f) =>
    f.to === oldPath || f.to.startsWith(`${oldPath}/`) ? { ...f, to: newPath + f.to.slice(oldPath.length) } : f,
  );
}

/** Starter files that end up inside a folder. */
export const filesInside = (files: FileEntry[] = [], path: string) =>
  files.filter((f) => f.to.startsWith(`${path}/`));

/** Every folder's path, so a rename/move can update starter files. */
export function pathsById(nodes: FolderNode[]): Map<string, string> {
  const out = new Map<string, string>();
  const walk = (list: FolderNode[], prefix: string) =>
    list.forEach((n) => {
      out.set(n.id, prefix + n.name);
      walk(n.kids, `${prefix}${n.name}/`);
    });
  walk(nodes, "");
  return out;
}

/** After an edit to the tree: move starter files whose folder changed path. */
export function followMoves(files: FileEntry[] = [], before: FolderNode[], after: FolderNode[]): FileEntry[] {
  const old = pathsById(before);
  const now = pathsById(after);
  // A path another folder already had: moving files there would mix both folders' files.
  const owner = new Map([...old.entries()].map(([id, path]) => [path, id]));
  let out = files;
  // Deepest first, so a moved parent doesn't hide its children's moves.
  [...old.entries()]
    .sort((a, b) => b[1].length - a[1].length)
    .forEach(([id, path]) => {
      const p = now.get(id);
      const taken = p !== undefined && owner.has(p) && owner.get(p) !== id;
      if (p && p !== path && !taken) out = movePathInFiles(out, path, p);
    });
  return out;
}

// ───────────────────────── Folder name blocks ─────────────────────────

export interface NameBlock {
  /** A question key, or a built-in like `jobCode`. */
  key: string;
  transform?: string;
}

/** `{start|yymmdd}_{name|caps}` → blocks joined by `sep`. `null` if the pattern has literal text
 *  or another shape: the editor then shows it as text under Advanced, so nothing is lost. */
export function toBlocks(pattern: string | undefined, sep = "_"): NameBlock[] | null {
  if (pattern === undefined || pattern === "") return [];
  const parts = pattern.split(sep);
  const blocks: NameBlock[] = [];
  for (const part of parts) {
    const m = /^\{([A-Za-z0-9_]+)(?:\|([a-z]+))?\}$/.exec(part);
    if (!m) return null;
    blocks.push(m[2] ? { key: m[1], transform: m[2] } : { key: m[1] });
  }
  return blocks;
}

export const fromBlocks = (blocks: NameBlock[], sep = "_") =>
  blocks.map((b) => `{${b.key}${b.transform ? `|${b.transform}` : ""}}`).join(sep);

/** How a block can be written, with an example of each. */
export function styleChoices(kind: FieldKind | "builtin"): { value: string; label: string }[] {
  if (kind === "date")
    return [
      { value: "yymmdd", label: "260302" },
      { value: "", label: "2026-03-02" },
    ];
  if (kind === "builtin") return [{ value: "", label: "As is" }];
  return [
    { value: "caps", label: "ALL_CAPS" },
    { value: "block", label: "ALLCAPS (one block)" },
    { value: "pascal", label: "OneBlock" },
    { value: "kebab", label: "lower-case" },
    { value: "", label: "As typed" },
  ];
}

export const BUILTIN_BLOCKS: { key: string; label: string }[] = [
  { key: "clientCode", label: "Client code" },
  { key: "jobCode", label: "Job code" },
  { key: "year", label: "Year" },
  { key: "yymm", label: "Year + month" },
];

// ───────────────────────── Questions ─────────────────────────

const RESERVED = new Set(["jobCode", "clientCode", "date", "yymmdd", "yymm", "year", "space", "templateName", "item"]);

/** A key for a new question, made from its label: letters, digits and _, unique, never a
 *  built-in. Renaming a question later never changes its key. */
export function newKey(label: string, taken: string[]): string {
  const base =
    label
      .normalize("NFKD")
      .replace(/[̀-ͯ]/g, "")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "")
      .slice(0, 30) || "question";
  const used = new Set(taken);
  for (let n = 1; ; n++) {
    const key = n === 1 ? base : `${base}_${n}`;
    if (!used.has(key) && !RESERVED.has(key)) return key;
  }
}

export interface Usage {
  inFolderName: boolean;
  inTitle: boolean;
  /** Folders whose "only if" or "one per" uses it. */
  folders: number;
  /** Folder names or starter files that use `{key}`. */
  names: number;
}

const usesVar = (s: string, key: string) => new RegExp(`\\{${key}(\\|[a-z]+)?\\}`).test(s);

export function usage(t: Template, key: string): Usage {
  const tree = t.tree ?? [];
  return {
    inFolderName: usesVar(t.folderName, key),
    inTitle: t.title !== undefined && usesVar(t.title, key),
    folders: tree.filter((e) => typeof e !== "string" && (strip(e.when ?? "") === key || e.each === key)).length,
    names:
      tree.filter((e) => usesVar(entryName(e), key)).length + (t.files ?? []).filter((f) => usesVar(f.to, key)).length,
  };
}

export const isUsed = (u: Usage) => u.inFolderName || u.inTitle || u.folders > 0 || u.names > 0;

/** Remove a question and everything that uses it: its blocks in the folder name and title,
 *  "only if" rules on folders (the folder stays, made always), "one per" folders (they need it),
 *  and folders or starter files named after it. */
export function removeQuestion(t: Template, key: string): Template {
  const dropVar = (pattern: string, sep: string) => {
    const blocks = toBlocks(pattern, sep);
    if (blocks) return fromBlocks(blocks.filter((b) => b.key !== key), sep);
    return pattern.replace(new RegExp(`${escape(sep)}?\\{${key}(\\|[a-z]+)?\\}`, "g"), "");
  };
  const tree: TreeEntry[] = [];
  for (const e of t.tree ?? []) {
    if (usesVar(entryName(e), key)) continue;
    if (typeof e === "string") {
      tree.push(e);
      continue;
    }
    if (e.each === key) continue;
    const when = e.when && strip(e.when) === key ? undefined : e.when;
    tree.push(when || e.each ? { name: e.name, ...(when ? { when } : {}), ...(e.each ? { each: e.each } : {}) } : e.name);
  }
  const title = t.title === undefined ? undefined : dropVar(t.title, " ");
  return {
    ...t,
    fields: t.fields.filter((f) => f.key !== key),
    folderName: dropVar(t.folderName, "_"),
    ...(title === undefined || title === "" ? { title: undefined } : { title }),
    tree,
    files: (t.files ?? []).filter((f) => !usesVar(f.to, key)),
  };
}

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** A sensible new question of a type. */
export function blankField(kind: FieldKind, taken: string[], label = "New question"): Field {
  const key = newKey(label, taken);
  if (kind === "multi" || kind === "select") return { key, label, type: kind, options: ["Option 1", "Option 2"] };
  if (kind === "bool") return { key, label, type: kind, default: false };
  if (kind === "date") return { key, label, type: kind, default: "today" };
  return { key, label, type: kind };
}

/** Change a question's type, dropping settings that don't fit the new type. */
export function retype(f: Field, kind: FieldKind): Field {
  // "Required" only fits answers that can be left empty (not yes/no or pick several).
  const keepsRequired = f.required && kind !== "bool" && kind !== "multi";
  const base: Field = { key: f.key, label: f.label, type: kind, ...(keepsRequired ? { required: true } : {}) };
  const options = f.options?.length ? f.options : ["Option 1", "Option 2"];
  // Starting picks carry over between "pick one" and "pick several".
  const picked = Array.isArray(f.default) ? f.default : typeof f.default === "string" ? [f.default] : [];
  const keep = picked.filter((o) => options.includes(o));
  if (kind === "multi") return { ...base, options, ...(keep.length ? { default: keep } : {}) };
  if (kind === "select") return { ...base, options, ...(keep.length ? { default: keep[0] } : {}) };
  if (kind === "bool") return { ...base, default: false, required: undefined };
  if (kind === "date") return { ...base, default: "today" };
  if ((kind === "text" || kind === "longtext") && f.max) return { ...base, max: f.max };
  return base;
}

/** The preview tree reads a Plan; an editor preview has what it needs. */
export const asPlan = (p: TemplatePreview): Plan =>
  ({
    folderName: p.folderName,
    folders: p.folders,
    files: p.files.map((to) => ({ from: "", to, fill: false })),
  }) as Plan;
