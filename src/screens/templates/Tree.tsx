import Icon from "../../components/Icon";
import { filesIn } from "../../lib/editorModel";
import { locate, type FolderNode } from "../../lib/templateDraft";
import type { Field, FileEntry } from "../../lib/types";
import NameField, { NameView } from "./NameField";
import { usePointerDrag } from "./drag";
import styles from "./editor.module.css";

/** A row's id: "root" (the project folder), a folder's node id, or "file:<from>". */
export type RowId = string;
export const ROOT = "root";
export const fileRow = (from: string) => `file:${from}`;

interface Props {
  folderName: string;
  /** The project folder's name with example answers (grey, next to it). */
  example: string;
  nodes: FolderNode[];
  files: FileEntry[];
  fields: Field[];
  selected: RowId | null;
  renaming: RowId | null;
  /** The folder files are being dragged over, as a template path ("" = the project folder). */
  dropPath: string | null;
  onSelect: (id: RowId) => void;
  onStartRename: (id: RowId) => void;
  onRename: (id: RowId, value: string, then?: "below" | "in" | "out") => void;
  onCancelRename: () => void;
  onAddInside: (folderId: string | null) => void;
  onMenu: (id: RowId) => void;
  onMove: (id: string, target: string, where: "before" | "after" | "inside") => void;
  onChip: (row: RowId, key: string, index: number) => void;
  onNewBlank: (label: string) => string;
}

/** Every row, top to bottom, as the tree shows them (for the arrow keys). */
export function rowOrder(nodes: FolderNode[], files: FileEntry[]): RowId[] {
  const out: RowId[] = [ROOT];
  const walk = (list: FolderNode[], prefix: string) =>
    list.forEach((n) => {
      out.push(n.id);
      const path = prefix + n.name;
      walk(n.kids, `${path}/`);
      filesIn(files, path).forEach((f) => out.push(fileRow(f.from)));
    });
  walk(nodes, "");
  filesIn(files, "").forEach((f) => out.push(fileRow(f.from)));
  return out;
}

const baseName = (to: string) => to.slice(to.lastIndexOf("/") + 1);

/** The template as an outline: the project folder on top, what's inside each folder below it
 *  with a line down the side, and "Add folder" inside every folder. */
export default function Tree(p: Props) {
  const fieldOf = (key: string) => p.fields.find((f) => f.key === key);
  const labelOf = (key: string) => fieldOf(key)?.label ?? key;

  const drag = usePointerDrag({
    attr: "data-folder-id",
    canDrop: (id, target) => {
      const from = locate(p.nodes, id);
      return id !== target && !!from && !locate(from.node.kids, target);
    },
    canHoldInside: (target) => !locate(p.nodes, target)?.node.each,
    onDrop: (id, t) => p.onMove(id, t.id, t.where),
  });

  const nameOrField = (id: RowId, value: string, label: string, opts: { below?: boolean; tab?: boolean } = {}) =>
    p.renaming === id ? (
      <NameField
        value={value}
        fields={p.fields}
        label={label}
        onCommit={(v) => p.onRename(id, v)}
        onCancel={p.onCancelRename}
        onEnter={opts.below ? (v) => p.onRename(id, v, "below") : undefined}
        onTab={opts.tab ? (v, out) => p.onRename(id, v, out ? "out" : "in") : undefined}
        onNewBlank={p.onNewBlank}
        onChip={(key) => p.onChip(id, key, -1)}
      />
    ) : (
      <NameView name={value} fields={p.fields} onChip={(key, i) => p.onChip(id, key, i)} />
    );

  const rowProps = (id: RowId) => ({
    role: "treeitem",
    "aria-selected": p.selected === id,
    onClick: () => p.onSelect(id),
    onDoubleClick: () => p.onStartRename(id),
    onContextMenu: (e: React.MouseEvent) => {
      e.preventDefault();
      p.onSelect(id);
      p.onMenu(id);
    },
  });

  const fileRows = (path: string) =>
    filesIn(p.files, path).map((f) => {
      const id = fileRow(f.from);
      return (
        <div key={id} className={`${styles.row} ${p.selected === id ? styles.selected : ""}`} {...rowProps(id)}>
          <Icon name="file" className={styles.fileIcon} />
          {nameOrField(id, baseName(f.to), "File name")}
          {f.fill && <span className={styles.tag}>fills in answers</span>}
        </div>
      );
    });

  const addRow = (parent: string | null) => (
    <div className={`${styles.row} ${styles.addRow}`} onClick={() => p.onAddInside(parent)}>
      + Add folder
    </div>
  );

  const folder = (n: FolderNode, path: string): React.ReactNode => {
    const id = n.id;
    const drop = drag.target?.id === id ? drag.target.where : null;
    const kids = n.kids.map((k) => folder(k, `${path}/${k.name}`));
    const files = fileRows(path);
    const empty = n.kids.length === 0 && files.length === 0;
    const each = n.each ? fieldOf(n.each) : undefined;
    return (
      <div key={id} className={`${styles.group} ${empty && !n.each ? styles.empty : ""}`}>
        <div
          data-folder-id={id}
          data-drop-path={n.each ? undefined : path}
          className={[
            styles.row,
            p.selected === id ? styles.selected : "",
            p.dropPath === path ? styles.dropTarget : "",
            drag.dragging === id ? styles.dragging : "",
            drop === "inside" ? styles.dropTarget : drop === "before" ? styles.dropBefore : drop === "after" ? styles.dropAfter : "",
          ].join(" ")}
          {...rowProps(id)}
          {...drag.handlers(id)}
        >
          <Icon name="folder" className={styles.folderIcon} />
          {n.each ? (
            <NameView name={`{${n.each}}`} fields={p.fields} onChip={(key) => p.onChip(id, key, -1)} />
          ) : (
            nameOrField(id, n.name, "Folder name", { below: true, tab: true })
          )}
          {n.each && <span className={styles.tag}>one folder for each {each ? `“${each.label}”` : "choice"}</span>}
          {n.when && (
            <span className={styles.tag}>
              only if {n.when.startsWith("!") ? `not “${labelOf(n.when.slice(1))}”` : `“${labelOf(n.when)}”`}
            </span>
          )}
        </div>
        <div className={styles.kids}>
          {kids}
          {files}
          {!n.each && addRow(id)}
        </div>
      </div>
    );
  };

  const top = p.nodes.map((n) => folder(n, n.name));
  const topFiles = fileRows("");

  return (
    <div role="tree" aria-label="Folders and files">
      <div className={`${styles.group} ${styles.always}`}>
        <div
          data-drop-path=""
          className={[styles.row, p.selected === ROOT ? styles.selected : "", p.dropPath === "" ? styles.dropTarget : ""].join(" ")}
          {...rowProps(ROOT)}
        >
          <Icon name="folder" className={styles.folderIcon} />
          {nameOrField(ROOT, p.folderName, "Project folder name")}
          {p.renaming !== ROOT && <span className={`${styles.example} ${styles.mono}`}>{p.example}</span>}
        </div>
        <div className={styles.kids}>
          {top}
          {topFiles}
          {addRow(null)}
        </div>
      </div>
    </div>
  );
}
