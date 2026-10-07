import { useState } from "react";
import { buildTree, type TreeNode } from "../lib/tree";
import type { Plan } from "../lib/types";
import Icon from "./Icon";
import { IconButton } from "./ui";
import styles from "./PreviewTree.module.css";

const ROOT = "";

/** Everything Create will make, straight from the Plan (build spec §3.2). Click a folder to open/close it. */
export default function PreviewTree({ plan, tall }: { plan: Plan; /** Grow with its column instead of a short box. */ tall?: boolean }) {
  // Closed folders by path, so they stay closed while the preview updates.
  const [closed, setClosed] = useState<Set<string>>(new Set());
  const tree = buildTree(plan);
  const fileCount = plan.files.length;

  const toggle = (path: string) =>
    setClosed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  const collapseAll = () => setClosed(new Set(plan.folders.filter((f) => !f.includes("/"))));

  return (
    <section className={styles.preview} aria-label="Preview">
      <div className={styles.head}>
        <h3 className={styles.title}>Preview</h3>
        <span className={styles.count}>
          {plan.folders.length} folder{plan.folders.length === 1 ? "" : "s"} · {fileCount} file
          {fileCount === 1 ? "" : "s"}
        </span>
        <IconButton icon="collapse" label="Collapse all folders" onClick={collapseAll} className={styles.collapse} />
      </div>
      <div className={tall ? `${styles.box} ${styles.tall}` : styles.box}>
        <ul className={styles.list} role="tree" aria-label={plan.folderName}>
          <Node
            node={{ name: plan.folderName || "…", isFile: false, children: tree }}
            path={ROOT}
            closed={closed}
            onToggle={toggle}
            root
          />
        </ul>
      </div>
    </section>
  );
}

interface NodeProps {
  node: TreeNode;
  path: string;
  closed: Set<string>;
  onToggle: (path: string) => void;
  root?: boolean;
}

function Node({ node, path, closed, onToggle, root }: NodeProps) {
  const hasKids = node.children.length > 0;
  const open = hasKids && !closed.has(path);
  return (
    <li role="treeitem" aria-expanded={hasKids ? open : undefined}>
      <div
        className={`${styles.row} ${root ? styles.root : ""}`}
        onClick={hasKids ? () => onToggle(path) : undefined}
      >
        {hasKids ? (
          <Icon name="chevron" className={`${styles.twisty} ${open ? styles.open : ""}`} />
        ) : (
          <span className={styles.twisty} />
        )}
        <Icon name={node.isFile ? "file" : "folder"} className={node.isFile ? styles.fileIcon : styles.folderIcon} />
        <span className={styles.name}>{node.name}</span>
      </div>
      {open && (
        <ul className={styles.list} role="group">
          {node.children.map((c) => {
            const childPath = path ? `${path}/${c.name}` : c.name;
            return (
              <Node
                key={`${c.isFile ? "f" : "d"}:${c.name}`}
                node={c}
                path={childPath}
                closed={closed}
                onToggle={onToggle}
              />
            );
          })}
        </ul>
      )}
    </li>
  );
}
