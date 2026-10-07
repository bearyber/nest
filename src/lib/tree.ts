import type { Field, Plan, Values } from "./types";

export interface TreeNode {
  name: string;
  isFile: boolean;
  children: TreeNode[];
}

/** Nest the Plan's flat `/` paths into a tree: folders first (plan order), then files. */
export function buildTree(plan: Pick<Plan, "folders" | "files">): TreeNode[] {
  const root: TreeNode = { name: "", isFile: false, children: [] };
  const insert = (path: string, isFile: boolean) => {
    const parts = path.split("/");
    let node = root;
    parts.forEach((name, i) => {
      const last = i === parts.length - 1;
      let child = node.children.find((c) => c.name === name && c.isFile === (last && isFile));
      if (!child) {
        child = { name, isFile: last && isFile, children: [] };
        node.children.push(child);
      }
      node = child;
    });
  };
  plan.folders.forEach((f) => insert(f, false));
  plan.files.forEach((f) => insert(f.to, true));
  return root.children;
}

/** Initial values for a template: each field's default, if it has one.
 *  A date field's `"today"` becomes today's local date (YYYY-MM-DD). */
export function defaultsFor(fields: Field[], now: Date = new Date()): Values {
  const values: Values = {};
  for (const f of fields) {
    if (f.default === undefined) continue;
    values[f.key] = f.type === "date" && f.default === "today" ? isoDate(now) : f.default;
  }
  return values;
}

/** Today's local date as YYYY-MM-DD. */
export const todayIso = () => isoDate(new Date());

function isoDate(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}
