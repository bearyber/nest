import type { Field, TreeEntry } from "./types";

/** A yes/no question that only decides whether some folders are made. */
export interface AlsoMake {
  field: Field;
  /** "PRINT folder", "PRINT and PROOFS folders". */
  label: string;
  /** The full paths, for the tooltip. */
  paths: string[];
}

function joinNames(names: string[]): string {
  if (names.length === 1) return `${names[0]} folder`;
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]} folders`;
}

/** The yes/no questions to show under "Also make:": a `bool` field with a tree rule
 *  `when: "<key>"`. A key that also has a `!<key>` rule ("made when NOT ticked") stays a
 *  normal question, and so does a yes/no no folder depends on. Template order. */
export function alsoMakeFor(fields: Field[], tree: TreeEntry[] = []): AlsoMake[] {
  const made = new Map<string, string[]>();
  const inverted = new Set<string>();
  for (const e of tree) {
    if (typeof e === "string" || !e.when) continue;
    if (e.when.startsWith("!")) {
      inverted.add(e.when.slice(1));
      continue;
    }
    made.set(e.when, [...(made.get(e.when) ?? []), e.name]);
  }
  const out: AlsoMake[] = [];
  for (const field of fields) {
    const paths = made.get(field.key);
    if (field.type !== "bool" || !paths || inverted.has(field.key)) continue;
    const last = paths.map((p) => p.split("/").filter(Boolean).pop() ?? p);
    const names = last.filter((n, i) => last.indexOf(n) === i);
    // A name with {blanks} isn't a name to read: use the question's own label.
    const label = names.some((n) => n.includes("{")) ? field.label : joinNames(names);
    out.push({ field, label, paths });
  }
  return out;
}
