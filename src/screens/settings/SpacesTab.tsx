import { useCallback, useEffect, useState } from "react";
import { Button, IconButton } from "../../components/ui";
import { addSpace, deleteSpace, moveSpace, renameSpace, setSpaceNoClient, spacesInUse } from "../../lib/commands";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

/** Spaces: add, reorder, mark "no client"; rename/delete only while no project uses it. */
export default function SpacesTab({ view, run }: TabProps) {
  const s = view.settings;
  const [used, setUsed] = useState<string[]>([]);
  const [newName, setNewName] = useState("");
  const [editing, setEditing] = useState<{ old: string; value: string } | null>(null);

  const loadUsed = useCallback(() => spacesInUse().then(setUsed).catch(() => setUsed([])), []);
  useEffect(() => {
    void loadUsed();
  }, [loadUsed, view]);

  const add = async () => {
    if ((await run(addSpace(newName))) !== undefined) setNewName("");
  };
  const saveRename = async () => {
    if (!editing) return;
    if ((await run(renameSpace(editing.old, editing.value))) !== undefined) setEditing(null);
  };

  return (
    <>
      <section className={styles.section}>
        <p className={styles.hint}>
          Spaces group your projects in the sidebar. In a "no client" space (like Personal) the Billed to field is
          hidden and job codes use your own code. A space that has projects can't be renamed or deleted, because
          projects keep their space name in their own files.
        </p>
        <div className={styles.card}>
          {s.spaces.map((name, i) => {
            const inUse = used.includes(name);
            const lockedWhy = inUse ? `"${name}" has projects` : undefined;
            return (
              <div key={name} className={styles.row}>
                {editing?.old === name ? (
                  <>
                    <input
                      className={`${styles.input} ${styles.grow}`}
                      value={editing.value}
                      autoFocus
                      aria-label={`New name for ${name}`}
                      onChange={(e) => setEditing({ old: name, value: e.target.value })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void saveRename();
                        if (e.key === "Escape") setEditing(null);
                      }}
                    />
                    <Button variant="primary" onClick={() => void saveRename()}>
                      Save
                    </Button>
                    <Button onClick={() => setEditing(null)}>Cancel</Button>
                  </>
                ) : (
                  <>
                    <span className={`${styles.grow} ${styles.name}`}>{name}</span>
                    {s.defaultSpace === name && <span className={styles.pillMuted}>Default</span>}
                    <label className={styles.check}>
                      <input
                        type="checkbox"
                        checked={s.noClientSpaces.includes(name)}
                        onChange={(e) => run(setSpaceNoClient(name, e.target.checked))}
                      />
                      No client
                    </label>
                    <IconButton
                      icon="down"
                      label="Move up"
                      disabled={i === 0}
                      style={{ transform: "rotate(180deg)" }}
                      onClick={() => run(moveSpace(name, i - 1))}
                    />
                    <IconButton
                      icon="down"
                      label="Move down"
                      disabled={i === s.spaces.length - 1}
                      onClick={() => run(moveSpace(name, i + 1))}
                    />
                    <Button disabled={inUse} title={lockedWhy} onClick={() => setEditing({ old: name, value: name })}>
                      Rename
                    </Button>
                    <Button
                      disabled={inUse || s.spaces.length === 1}
                      title={lockedWhy ?? (s.spaces.length === 1 ? "Keep at least one space" : undefined)}
                      onClick={() => run(deleteSpace(name), `Deleted "${name}"`)}
                    >
                      Delete
                    </Button>
                  </>
                )}
              </div>
            );
          })}
        </div>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Add a space</h3>
        <div className={styles.actions}>
          <input
            className={styles.input}
            placeholder="e.g. Look dev"
            value={newName}
            aria-label="New space name"
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void add()}
          />
          <Button disabled={!newName.trim()} onClick={() => void add()}>
            Add
          </Button>
        </div>
      </section>
    </>
  );
}
