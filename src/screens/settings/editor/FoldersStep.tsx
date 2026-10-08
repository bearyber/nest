import { useRef, useState } from "react";
import Icon from "../../../components/Icon";
import { Button } from "../../../components/ui";
import {
  cloneNodes,
  filesInside,
  followMoves,
  locate,
  moveNode,
  nodeId,
  type FolderNode,
} from "../../../lib/templateDraft";
import type { FileEntry, Template } from "../../../lib/types";
import { usePointerDrag } from "./drag";
import styles from "./editor.module.css";

interface Props {
  draft: Template;
  nodes: FolderNode[];
  starterFiles: string[];
  /** The project folder name with example answers (from the preview). */
  folderName: string;
  onChange: (nodes: FolderNode[], files?: FileEntry[]) => void;
  onFieldsChange: (t: Template) => void;
}

/** The "for each choice" folder inside a folder, if it has one. */
const eachChild = (n: FolderNode) => n.kids.find((k) => k.each);

/** "2 folders and 1 file", for the delete question. */
function insideText(n: FolderNode, fileCount: number): string {
  const folders = n.kids.length;
  const parts = [];
  if (folders) parts.push(`${folders} folder${folders === 1 ? "" : "s"}`);
  if (fileCount) parts.push(`${fileCount} file${fileCount === 1 ? "" : "s"}`);
  return parts.join(" and ");
}

/** Step 3: the folders inside the project. The tree shows what will be made (with example
 *  answers); click a folder to change it in the panel on the right. Drag to move. */
export default function FoldersStep({ draft, nodes, folderName, onChange }: Props) {
  const [selected, setSelected] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  /** The tree when the name box got focus: starter files follow the rename on blur. */
  const renameFrom = useRef<FolderNode[] | null>(null);
  const ticks = draft.fields.filter((f) => f.type === "bool");
  const picks = draft.fields.filter((f) => f.type === "multi");
  const field = (key: string) => draft.fields.find((f) => f.key === key);
  const labelOf = (key: string) => field(key)?.label ?? key;
  const files = draft.files ?? [];

  /** Apply a change; starter files follow folders that moved or were renamed. */
  const commit = (next: FolderNode[]) => onChange(next, followMoves(files, nodes, next));
  const edit = (id: string, change: (n: FolderNode, next: FolderNode[]) => void) => {
    const next = cloneNodes(nodes);
    const at = locate(next, id);
    if (!at) return;
    change(at.node, next);
    commit(next);
  };

  const drag = usePointerDrag({
    attr: "data-folder-id",
    canDrop: (id, target) => {
      const from = locate(nodes, id);
      return id !== target && !!from && !locate(from.node.kids, target);
    },
    canHoldInside: () => true,
    onDrop: (id, t) => {
      commit(moveNode(nodes, id, t.id, t.where));
      setSelected(id);
    },
  });

  const add = (parent: string | null) => {
    const next = cloneNodes(nodes);
    const at = parent ? locate(next, parent) : null;
    const node: FolderNode = { id: nodeId(), name: "NEW_FOLDER", explicit: true, kids: [] };
    (at ? at.node.kids : next).push(node);
    commit(next);
    setSelected(node.id);
    // Put the cursor in the name box so you can type the name straight away.
    setTimeout(() => {
      const input = document.getElementById("folder-name") as HTMLInputElement | null;
      input?.focus();
      input?.select();
    }, 0);
  };

  const remove = (id: string, sure = false) => {
    const at = locate(nodes, id);
    if (!at) return;
    // Ask first when anything is inside (folders or starter files): there's no undo.
    if ((at.node.kids.length > 0 || filesInside(files, at.path).length > 0) && !sure) {
      setConfirmDelete(id);
      return;
    }
    const next = cloneNodes(nodes);
    const n = locate(next, id)!;
    n.list.splice(n.index, 1);
    onChange(next, files.filter((f) => !f.to.startsWith(`${at.path}/`)));
    setSelected(null);
    setConfirmDelete(null);
  };

  const shift = (id: string, by: -1 | 1) =>
    edit(id, (_, next) => {
      const n = locate(next, id)!;
      const to = n.index + by;
      if (to >= 0 && to < n.list.length) n.list.splice(to, 0, n.list.splice(n.index, 1)[0]);
    });

  /** Turn "a folder for each choice" on or off inside a folder. */
  const setEach = (id: string, key: string | null) =>
    edit(id, (n) => {
      const existing = eachChild(n);
      if (key && existing) existing.each = key;
      else if (key) n.kids.push({ id: nodeId(), name: "{item}", each: key, explicit: true, kids: [] });
      else n.kids = n.kids.filter((k) => !k.each);
      n.explicit = true;
    });
  /** Turning "for each choice" off also drops starter files that lived inside those folders. */
  const clearEach = (id: string) => {
    const at = locate(nodes, id);
    const child = at && eachChild(at.node);
    if (!at || !child) return;
    const next = cloneNodes(nodes);
    const n = locate(next, id)!;
    n.node.kids = n.node.kids.filter((k) => !k.each);
    const prefix = `${at.path}/${child.name}/`;
    onChange(next, files.filter((f) => !f.to.startsWith(prefix)));
  };

  const whenLabel = (when: string) =>
    when.startsWith("!") ? `if not “${labelOf(when.slice(1))}”` : `if “${labelOf(when)}”`;

  const rows: React.ReactNode[] = [];
  const walk = (list: FolderNode[], depth: number) =>
    list.forEach((n) => {
      if (n.each) {
        // Show the folders this makes, from the question's choices (picked ones first).
        const f = field(n.each);
        const choices = f?.options ?? [];
        choices.forEach((o, i) =>
          rows.push(
            <div
              key={`${n.id}-${o}`}
              className={`${styles.row} ${styles.madeRow}`}
              style={{ paddingLeft: 8 + depth * 22 + 14 }}
              onClick={() => setSelected(parentOf(n.id) ?? n.id)}
            >
              <Icon name="folder" className={styles.folderIconMuted} />
              <span className={`${styles.rowName} ${styles.mono}`}>
                {o}
                {i === 0 && <em> one for each choice picked in {f?.label ?? "the question"}</em>}
              </span>
            </div>,
          ),
        );
        walk(n.kids, depth + 1);
        return;
      }
      const drop = drag.target?.id === n.id ? drag.target.where : null;
      rows.push(
        <div
          key={n.id}
          data-folder-id={n.id}
          role="treeitem"
          aria-selected={selected === n.id}
          tabIndex={0}
          className={[
            styles.row,
            selected === n.id ? styles.rowSelected : "",
            drag.dragging === n.id ? styles.dragging : "",
            drop === "inside" ? styles.dropInside : drop === "before" ? styles.dropBefore : drop === "after" ? styles.dropAfter : "",
          ].join(" ")}
          style={{ paddingLeft: 8 + depth * 22 }}
          onClick={() => setSelected(n.id)}
          onKeyDown={(e) => {
            if (e.target !== e.currentTarget) return;
            if (e.key === "F2" || e.key === "Enter") document.getElementById("folder-name")?.focus();
          }}
          {...drag.handlers(n.id)}
        >
          <span className={styles.grip} aria-hidden="true" title="Drag to move">
            ⠿
          </span>
          <Icon name="folder" className={styles.folderIcon} />
          <span className={`${styles.rowNameShrink} ${styles.mono}`}>{n.name}</span>
          {n.when && <span className={styles.tag}>{whenLabel(n.when)}</span>}
        </div>,
      );
      walk(n.kids, depth + 1);
    });
  walk(nodes, 1);

  function parentOf(id: string): string | null {
    let found: string | null = null;
    const go = (list: FolderNode[], parent: string | null) =>
      list.forEach((n) => {
        if (n.id === id) found = parent;
        go(n.kids, n.id);
      });
    go(nodes, null);
    return found;
  }

  const sel = selected ? locate(nodes, selected) : null;
  const node = sel?.node;
  const each = node ? eachChild(node) : undefined;

  return (
    <div className={styles.folders}>
      <div className={styles.section}>
        <div className={styles.rowBetween}>
          <span className={styles.label}>Folders inside the project</span>
          <Button onClick={() => add(null)}>+ Add folder</Button>
        </div>
        <div className={styles.tree} role="tree" aria-label="Folders">
          <div className={`${styles.row} ${styles.rootRow}`} style={{ paddingLeft: 8 }}>
            <Icon name="folder" className={styles.folderIcon} />
            <span className={`${styles.rowName} ${styles.mono}`}>
              <b>{folderName || "Project folder"}</b>
            </span>
          </div>
          {rows}
          {nodes.length === 0 && <p className={styles.empty}>No folders yet. Click “+ Add folder”.</p>}
          {files
            .filter((f) => !f.to.includes("/"))
            .map((f) => (
              <div key={f.to} className={`${styles.row} ${styles.fileRow}`} style={{ paddingLeft: 30 }}>
                <Icon name="file" className={styles.fileIcon} />
                <span className={`${styles.rowName} ${styles.mono}`}>
                  {f.to}
                  <em> file that comes with the template</em>
                </span>
              </div>
            ))}
        </div>
        <p className={styles.hint}>Click a folder to change it. Drag ⠿ to move it; drop it on another folder to put it inside.</p>
      </div>

      <aside className={styles.panel} aria-label="Selected folder">
        {!node ? (
          <p className={styles.panelEmpty}>Click a folder on the left to rename it, move it, or choose when it's made.</p>
        ) : node.each ? (
          // A "for each choice" folder at the top level (only from an imported template).
          <>
            <p className={styles.hint}>
              One folder for each choice picked in “{labelOf(node.each)}”, made straight inside the project folder.
            </p>
            <div className={styles.panelActions}>
              <Button onClick={() => remove(node.id)}>Remove these folders</Button>
            </div>
          </>
        ) : (
          <>
            <div className={styles.section}>
              <label className={styles.label} htmlFor="folder-name">
                Folder name
              </label>
              <input
                id="folder-name"
                className={`${styles.input} ${styles.mono}`}
                value={node.name}
                maxLength={80}
                spellCheck={false}
                onFocus={() => (renameFrom.current = nodes)}
                onChange={(e) => {
                  // The name changes as you type; starter files follow once you're done (on blur),
                  // so passing through another folder's name never mixes their files up.
                  const name = e.target.value.replace(/[/\\]/g, "");
                  const next = cloneNodes(nodes);
                  const at = locate(next, node.id);
                  if (at) at.node.name = name;
                  onChange(next);
                }}
                onBlur={(e) => {
                  const next = cloneNodes(nodes);
                  const at = locate(next, node.id);
                  if (at && !e.target.value.trim()) at.node.name = "NEW_FOLDER";
                  onChange(next, renameFrom.current ? followMoves(files, renameFrom.current, next) : files);
                  renameFrom.current = null;
                }}
              />
            </div>

            <fieldset className={styles.fieldset}>
              <legend className={styles.label}>When to make it</legend>
              <label className={styles.radio}>
                <input type="radio" name="when" checked={!node.when} onChange={() => edit(node.id, (n) => (n.when = undefined))} />
                Always
              </label>
              {ticks.map((f) => (
                <span key={f.key} className={styles.radioGroup}>
                  <label className={styles.radio}>
                    <input
                      type="radio"
                      name="when"
                      checked={node.when === f.key}
                      onChange={() => edit(node.id, (n) => ((n.when = f.key), (n.explicit = true)))}
                    />
                    Only if “{f.label}” is ticked
                  </label>
                  <label className={styles.radio}>
                    <input
                      type="radio"
                      name="when"
                      checked={node.when === `!${f.key}`}
                      onChange={() => edit(node.id, (n) => ((n.when = `!${f.key}`), (n.explicit = true)))}
                    />
                    Only if “{f.label}” is not ticked
                  </label>
                </span>
              ))}
              {ticks.length === 0 && (
                <p className={styles.hint}>To make a folder optional, add a “Yes / no” question in step 1, Questions (like “Has graphics”).</p>
              )}
            </fieldset>

            <fieldset className={styles.fieldset}>
              <legend className={styles.label}>Folders inside, one for each choice</legend>
              {picks.length === 0 ? (
                <p className={styles.hint}>
                  Add a “Pick several” question in step 1, Questions (like Formats: 16x9, 9x16…) to make a folder for each choice
                  picked.
                </p>
              ) : (
                <>
                  <label className={styles.radio}>
                    <input type="radio" name="each" checked={!each} onChange={() => clearEach(node.id)} />
                    None
                  </label>
                  {picks.map((f) => (
                    <label key={f.key} className={styles.radio}>
                      <input
                        type="radio"
                        name="each"
                        checked={each?.each === f.key}
                        onChange={() => setEach(node.id, f.key)}
                      />
                      <span>
                        A folder for each choice picked in “{f.label}”
                        <span className={styles.hintInline}>
                          {" "}
                          ({(f.options ?? []).slice(0, 3).join(", ")}
                          {(f.options?.length ?? 0) > 3 ? " …" : ""})
                        </span>
                      </span>
                    </label>
                  ))}
                </>
              )}
            </fieldset>

            <div className={styles.panelActions}>
              <Button onClick={() => add(node.id)}>Add a folder inside</Button>
              <Button disabled={sel!.index === 0} onClick={() => shift(node.id, -1)}>
                Move up
              </Button>
              <Button disabled={sel!.index >= sel!.list.length - 1} onClick={() => shift(node.id, 1)}>
                Move down
              </Button>
              <Button onClick={() => remove(node.id)}>Delete folder</Button>
            </div>
            {confirmDelete === node.id && (
              <div className={styles.confirm} role="alert">
                <span>
                  “{node.name}” has {insideText(node, filesInside(files, sel!.path).length)} inside. Delete it all from the
                  template?
                </span>
                <Button onClick={() => setConfirmDelete(null)}>Keep it</Button>
                <Button onClick={() => remove(node.id, true)}>Delete</Button>
              </div>
            )}
          </>
        )}
      </aside>
    </div>
  );
}
