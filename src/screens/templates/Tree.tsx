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
  /** Each name as typed → as the example project has it (from Rust). Missing: shown as typed. */
  examples: Record<string, string>;
  nodes: FolderNode[];
  files: FileEntry[];
  fields: Field[];
  selected: RowId | null;
  renaming: RowId | null;
  /** A folder just added: it unfolds into place, its name selected, ready to type over. */
  fresh: RowId | null;
  /** The folder files are being dragged over, as a template path ("" = the project folder). */
  dropPath: string | null;
  onSelect: (id: RowId) => void;
  onStartRename: (id: RowId) => void;
  onRename: (id: RowId, value: string, then?: "below" | "in" | "out") => void;
  onCancelRename: () => void;
  onAddInside: (folderId: string | null) => void;
  /** "+ Add a file" in a folder (template path; "" = the project folder). */
  onAddFile: (path: string) => void;
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

export const baseName = (to: string) => to.slice(to.lastIndexOf("/") + 1);

/** Every name the tree shows (project folder, folders, files), once each: what to ask Rust for
 *  examples of. */
export function visibleNames(folderName: string, nodes: FolderNode[], files: FileEntry[]): string[] {
  const out = new Set<string>([folderName]);
  const walk = (list: FolderNode[]) =>
    list.forEach((n) => {
      out.add(n.name);
      walk(n.kids);
    });
  walk(nodes);
  files.forEach((f) => out.add(baseName(f.to)));
  return [...out];
}

/** The template as an outline: the project folder on top, what's inside each folder below it
 *  with a line down the side, and "Add folder" inside every folder. Names show as the example
 *  project has them; click one to change it. */
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

  /** A name as the example project has it. Before Rust has answered: as typed, with chips. */
  const example = (id: RowId, value: string, renamable: boolean) => {
    const shown = p.examples[value];
    if (shown === undefined) return <NameView name={value} fields={p.fields} onChip={(key, i) => p.onChip(id, key, i)} />;
    return (
      <span
        className={`${styles.name} ${styles.mono} ${renamable ? styles.clickName : ""}`}
        title={shown !== value ? value : undefined}
        onClick={
          renamable
            ? (e) => {
                e.stopPropagation();
                p.onStartRename(id);
              }
            : undefined
        }
      >
        {shown}
      </span>
    );
  };

  const nameOrField = (id: RowId, value: string, label: string, opts: { below?: boolean; tab?: boolean } = {}) =>
    p.renaming === id ? (
      <NameField
        value={value}
        fields={p.fields}
        label={label}
        selectAll={p.fresh === id}
        onCommit={(v) => p.onRename(id, v)}
        onCancel={p.onCancelRename}
        onEnter={opts.below ? (v) => p.onRename(id, v, "below") : undefined}
        onTab={opts.tab ? (v, out) => p.onRename(id, v, out ? "out" : "in") : undefined}
        onNewBlank={p.onNewBlank}
        onChip={(key, i) => p.onChip(id, key, i)}
      />
    ) : (
      example(id, value, true)
    );

  /** ⋯ on the row under the pointer: the same menu as right-click. */
  const more = (id: RowId, what: string) =>
    p.renaming === id ? null : (
      <button
        type="button"
        className={styles.rowMore}
        aria-label={`More for ${what}`}
        title="More"
        onClick={(e) => {
          e.stopPropagation();
          p.onSelect(id);
          p.onMenu(id);
        }}
      >
        ⋯
      </button>
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
        <div
          key={id}
          className={`${styles.row} ${p.selected === id ? styles.selected : ""}`}
          title={f.fill ? "New projects get this file with New Project's answers filled in." : undefined}
          {...rowProps(id)}
        >
          <Icon name="file" className={styles.fileIcon} />
          {nameOrField(id, baseName(f.to), "File name")}
          {more(id, baseName(f.to))}
        </div>
      );
    });

  const addRow = (parent: string | null, path: string, empty: boolean) => (
    <div className={`${styles.row} ${styles.addRow}`} data-drop-path={path}>
      <button type="button" className={styles.addLink} onClick={() => p.onAddInside(parent)}>
        + Add folder
      </button>
      <button type="button" className={styles.addLink} onClick={() => p.onAddFile(path)}>
        + Add a file
      </button>
      {empty && <span className={styles.dropHint}>or drop files here</span>}
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
      <div
        key={id}
        className={`${styles.group} ${empty && !n.each ? styles.empty : ""} ${p.fresh === id ? "nest-unfold" : ""}`}
      >
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
          {n.each ? example(id, n.name, false) : nameOrField(id, n.name, "Folder name", { below: true, tab: true })}
          {n.each && <span className={styles.tag}>one folder for each {each ? `“${each.label}”` : "choice"}</span>}
          {n.kit && <span className={styles.tag}>client's files</span>}
          {n.when && (
            <span className={styles.tag}>
              only if {n.when.startsWith("!") ? `not “${labelOf(n.when.slice(1))}”` : `“${labelOf(n.when)}”`}
            </span>
          )}
          {more(id, n.name)}
        </div>
        <div className={styles.kids}>
          {kids}
          {files}
          {!n.each && addRow(id, path, empty)}
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
          {more(ROOT, "the project folder")}
        </div>
        <div className={styles.kids}>
          {top}
          {topFiles}
          {addRow(null, "", p.nodes.length === 0 && topFiles.length === 0)}
        </div>
      </div>
    </div>
  );
}
